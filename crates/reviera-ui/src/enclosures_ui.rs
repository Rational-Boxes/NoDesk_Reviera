// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Glazed Enclosure tool (ARC-GLZ-001/002): Architecture › Glazed Enclosure asks for the form
//! and its sizes, then either places it at the typed corner or lets the user click the corner and
//! the direction of the length in a plan (`enclosure.create`). Selecting an enclosure or one of
//! its parts shows the enclosure's rows in Properties, which rebuild it (`enclosure.set`).

use reviera_core::ElementId;
use reviera_core::enclosures::{self, Form};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const FORMS: [(Form, &str); 6] = [
    (Form::Gable, "Gable"),
    (Form::Multispan, "Gutter-connected multispan"),
    (Form::Venlo, "Venlo"),
    (Form::Sawtooth, "Sawtooth"),
    (Form::Hoop, "Hoop tunnel"),
    (Form::Gothic, "Gothic tunnel"),
];

/// The screen sets offered in Properties.
const SCREEN_SETS: [(&str, &[&str]); 5] = [
    ("None", &[]),
    ("Energy", &["energy"]),
    ("Shade", &["shade"]),
    ("Energy and shade", &["energy", "shade"]),
    ("Energy, shade and blackout", &["energy", "shade", "blackout"]),
];

const ACTUATORS: [(reviera_core::vents::Actuator, &str); 4] = [
    (reviera_core::vents::Actuator::Manual, "Manual"),
    (reviera_core::vents::Actuator::RackAndPinion, "Rack and pinion"),
    (reviera_core::vents::Actuator::Chain, "Chain"),
    (reviera_core::vents::Actuator::Linear, "Linear"),
];

/// The tool placing an enclosure by two clicks: its arguments, and the corner once clicked.
#[derive(Debug, Clone)]
pub struct EnclosureTool {
    pub args: Value,
    pub corner: Option<[f64; 2]>,
}

impl Gui {
    /// Architecture › Glazed Enclosure.
    pub(crate) fn enclosure_dialog(&mut self) {
        let frame = self.frame;
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else {
            return frame.set_status_text("Glazed Enclosure: open a plan of the level it stands on", 0);
        };
        let dialog = Dialog::builder(&frame, "Glazed Enclosure").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let note = StaticText::builder(&dialog)
            .with_label("A greenhouse made of sloped-glazing roofs, glazed walls, knee walls and gutters. Change it later in Properties.")
            .build();
        note.wrap(380);
        sizer.add(&note, 0, SizerFlag::All, 10);
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        grid.add_growable_col(1, 1);
        let l = StaticText::builder(&dialog).with_label("Form").build();
        let form = Choice::builder(&dialog).with_choices(FORMS.iter().map(|f| f.1.to_owned()).collect()).build();
        form.set_selection(2);
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&form, 1, SizerFlag::Expand, 0);
        let field = |label: &str, value: &str| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let t = TextCtrl::builder(&dialog).with_value(value).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&t, 1, SizerFlag::Expand, 0);
            t
        };
        // The Venlo defaults (provisional, EXPERT_REVIEW E-44); blank keeps the form's own.
        let length = field("Length (along the ridges)", "40 m");
        let span = field("Span (Venlo: truss width)", "8 m");
        let spans = field("Spans (Venlo: trusses)", "6");
        let bay = field("Bay spacing", "4 m");
        let eave = field("Eave (gutter) height", "5.5 m");
        let pitch = field("Roof pitch", "22°");
        let knee = field("Knee wall height (0 for none)", "0");
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let click = CheckBox::builder(&dialog).with_label("Place by clicking the corner, then the direction of the length").build();
        click.set_value(true);
        sizer.add(&click, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        dialog.set_escape_id(ID_CANCEL);
        let code = dialog.show_modal();
        let chosen = form.get_selection().and_then(|i| FORMS.get(i as usize)).map(|f| f.0);
        let mut args = json!({ "form": chosen, "level_id": level });
        for (key, t) in [("length", &length), ("span", &span), ("bay", &bay), ("eave", &eave), ("pitch", &pitch), ("knee", &knee)] {
            let v = t.get_value();
            if !v.trim().is_empty() {
                args[key] = json!(v.trim());
            }
        }
        if let Ok(n) = spans.get_value().trim().parse::<u32>() {
            args["spans"] = json!(n);
        }
        let by_click = click.get_value();
        dialog.destroy();
        if code != ID_OK {
            return;
        }
        // One span, and a tunnel takes the form's own sizes for what only a range has.
        if matches!(chosen, Some(Form::Gable | Form::Hoop | Form::Gothic)) {
            args.as_object_mut().expect("an object").remove("spans");
        }
        if matches!(chosen, Some(Form::Hoop | Form::Gothic)) {
            let o = args.as_object_mut().expect("an object");
            for k in ["pitch", "bay"] {
                o.remove(k);
            }
            if o.get("span").and_then(Value::as_str) == Some("8 m") {
                o.remove("span");
            }
            if o.get("eave").and_then(Value::as_str) == Some("5.5 m") {
                o.remove("eave");
            }
        }
        // A sawtooth takes its own span, height and slope for the Venlo's left unchanged.
        if chosen == Some(Form::Sawtooth) {
            let o = args.as_object_mut().expect("an object");
            for (k, venlo) in [("span", "8 m"), ("eave", "5.5 m"), ("pitch", "22°")] {
                if o.get(k).and_then(Value::as_str) == Some(venlo) {
                    o.remove(k);
                }
            }
        }
        if by_click {
            self.enclosure_tool = Some(EnclosureTool { args, corner: None });
            frame.set_status_text("Glazed Enclosure: click its corner; Esc cancels", 0);
        } else {
            self.make_enclosure(args);
        }
    }

    pub(crate) fn end_enclosure_tool(&mut self) -> bool {
        self.enclosure_tool.take().is_some()
    }

    /// A click of the enclosure tool: the corner, then a point the length runs toward.
    pub(crate) fn enclosure_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(tool) = self.enclosure_tool.as_mut() else { return };
        match tool.corner {
            None => {
                tool.corner = Some(p);
                frame.set_status_text("Glazed Enclosure: click toward where its length runs", 0);
            }
            Some(c) => {
                let mut args = tool.args.clone();
                self.enclosure_tool = None;
                args["origin"] = json!(c);
                args["rotation"] = json!((p[1] - c[1]).atan2(p[0] - c[0]));
                self.make_enclosure(args);
            }
        }
    }

    fn make_enclosure(&mut self, args: Value) {
        let frame = self.frame;
        match self.run("enclosure.create", args) {
            Ok(v) => {
                let _ = self.run("selection.set", json!({ "ids": [v] }));
                self.selection_changed();
                frame.set_status_text("Glazed enclosure made: change it in Properties", 0);
            }
            Err(e) => error(&frame, &format!("The enclosure was not made:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// The enclosure of a selected element: the enclosure itself or one of its parts.
    fn enclosure_of_selection(&self, id: ElementId) -> Option<(ElementId, enclosures::EnclosureRecipe)> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(id)?;
        let owner = match enclosures::enclosure_of(e) {
            Some(g) => store.find_by_guid(&reviera_core::GlobalId::parse(g)?)?,
            None => id,
        };
        Some((owner, enclosures::recipe(store.get(owner)?)?))
    }

    /// The enclosure's rows, for a selected enclosure or part.
    pub(crate) fn enclosure_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some((_, r)) = self.enclosure_of_selection(id) else { return };
        grid.append(Property::category("Glazed Enclosure", "c_enclosure"));
        let form = FORMS.iter().find(|f| f.0 == r.form).map_or("", |f| f.1);
        grid.append(Property::string("Form", "gh_form", form.to_owned()));
        grid.set_property_read_only("gh_form", true);
        let venlo = r.form == Form::Venlo;
        let peaks = r.peaks.unwrap_or(2).max(1);
        let (span, spans) = if venlo { (r.span * f64::from(peaks), r.spans / peaks) } else { (r.span, r.spans) };
        let angle = |g: &mut Gui, a: f64| {
            g.run("units.format", json!({ "value": a, "data_type": "angle" })).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| format!("{:.1}°", a.to_degrees()))
        };
        let rows = [
            ("Length", "gh_length", self.format_length(r.length)),
            (if venlo { "Truss Width" } else { "Span" }, "gh_span", self.format_length(span)),
            (if venlo { "Trusses" } else { "Spans" }, "gh_spans", spans.to_string()),
            ("Bay Spacing", "gh_bay", self.format_length(r.bay)),
            ("Eave Height", "gh_eave", self.format_length(r.eave)),
            ("Roof Pitch", "gh_pitch", angle(self, r.pitch)),
            ("Knee Wall", "gh_knee", self.format_length(r.knee.unwrap_or(0.0))),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        if r.form == Form::Gable {
            grid.set_property_read_only("gh_spans", true);
        }
        // Glazing from the library (ARC-GLZ-003); the first choice is the greenhouse default.
        let names: Vec<String> = std::iter::once("Greenhouse default".to_owned()).chain(reviera_core::glazing_library::library().iter().map(|e| e.name.to_owned())).collect();
        let at = |g: &Option<String>| g.as_ref().and_then(|g| names.iter().position(|n| n.eq_ignore_ascii_case(g))).unwrap_or(0) as i32;
        let choices = || names.iter().enumerate().map(|(i, n)| (n.clone(), i as i32)).collect::<Vec<_>>();
        grid.append(Property::enumeration("Roof Glazing", "gh_roof_glazing", choices(), at(&r.roof_glazing)));
        grid.append(Property::enumeration("Wall Glazing", "gh_wall_glazing", choices(), at(&r.wall_glazing)));
        grid.set_help_string("gh_roof_glazing", "The glazing build-up (light, U-value, safety) of the roof; glazing.list_types lists them");
        let frames: Vec<String> = std::iter::once("Greenhouse default".to_owned()).chain(reviera_core::frame_library::library().iter().map(|e| e.name.to_owned())).collect();
        let at_frame = r.frame_system.as_ref().and_then(|f| frames.iter().position(|n| n.eq_ignore_ascii_case(f))).unwrap_or(0) as i32;
        grid.append(Property::enumeration("Frame System", "gh_frame_system", frames.iter().enumerate().map(|(i, n)| (n.clone(), i as i32)).collect::<Vec<_>>(), at_frame));
        grid.set_help_string("gh_frame_system", "The glazing bars' material, section and deflection limit; frame.list_types lists them");
        grid.append(Property::boolean("Gutters", "gh_gutters", r.gutters));
        grid.append(Property::boolean("Doors", "gh_doors", r.doors));
        grid.append(Property::boolean("Frame", "gh_frame", r.frame));
        grid.set_help_string("gh_frame", "Steel posts at every bay on each gutter line");
        let saw = r.form == Form::Sawtooth;
        grid.append(Property::string(if saw { "Face Vents (% of floor)" } else { "Ridge Vents (% of floor)" }, "gh_vents", format!("{}", r.vents)));
        grid.set_help_string("gh_vents", if saw { "The vent band along each vertical face's top, as a share of the span's floor; 0 for none" } else { "The ridge vents' target area as a share of the floor area; 0 for none" });
        grid.set_help_string("gh_doors", "A door in the middle of each end wall");
        if !r.form.tunnel() {
            grid.append(Property::string("Side Vents (% of floor)", "gh_side_vents", format!("{}", r.side_vents)));
            grid.set_help_string("gh_side_vents", "Vent bands along the side walls' tops, as a share of the floor area; 0 for none");
        }
        grid.append(Property::boolean("Headhouse", "gh_headhouse", r.headhouse.is_some()));
        grid.set_help_string("gh_headhouse", "A solid-walled workroom across one end, with a door; the end wall's door into the range widened to 3 m");
        if let Some(h) = r.headhouse {
            let (d, ht) = (self.format_length(h.depth), self.format_length(h.height));
            grid.append(Property::string("Headhouse Depth", "gh_hh_depth", d));
            grid.append(Property::string("Headhouse Height", "gh_hh_height", ht));
            let at = if h.at == enclosures::End::End { 1 } else { 0 };
            grid.append(Property::enumeration("Headhouse At", "gh_hh_at", vec![("Start".to_owned(), 0), ("End".to_owned(), 1)], at));
        }
        let roll = self.format_length(r.roll_up);
        grid.append(Property::string("Roll-up Sides", "gh_roll_up", roll));
        grid.set_help_string("gh_roll_up", "How far up the side walls open, their film rolled up; 0 for none");
        let gable = self.format_length(r.gable_vents);
        grid.append(Property::string("Gable Vents", "gh_gable_vents", gable));
        grid.set_help_string("gh_gable_vents", "A band of vents this deep under the top of each end wall, kept above the doors; 0 for none");
        let opening = angle(self, r.vent.opening_angle);
        grid.append(Property::string("Vent Opening Angle", "gh_vent_angle", opening));
        grid.set_help_string("gh_vent_angle", "The vents' design opening angle, for their free area");
        let at = ACTUATORS.iter().position(|a| a.0 == r.vent.actuator).unwrap_or(0) as i32;
        grid.append(Property::enumeration("Vent Actuator", "gh_vent_actuator", ACTUATORS.iter().enumerate().map(|(i, a)| (a.1.to_owned(), i as i32)).collect::<Vec<_>>(), at));
        let ridge = self.format_length(r.ridge());
        grid.append(Property::string("Ridge Height", "gh_ridge", ridge));
        grid.set_property_read_only("gh_ridge", true);
        let area = r.width() * r.length;
        grid.append(Property::string("Floor Area", "gh_area", format!("{area:.1} m²")));
        grid.set_property_read_only("gh_area", true);
        let vents = self.enclosure_of_selection(id).and_then(|(owner, _)| self.run("enclosure.get", json!({ "enclosure_id": owner })).ok());
        if let Some(v) = vents {
            let (a, pc) = (v["vent_area"].as_f64().unwrap_or(0.0), v["vent_percent"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Vent Area", "gh_vent_area", format!("{a:.1} m² ({pc:.1} %)")));
            grid.set_property_read_only("gh_vent_area", true);
            let (free, eff) = (v["free_vent_area"].as_f64().unwrap_or(0.0), v["effective_vent_area"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Free / Effective Vent Area", "gh_vent_free", format!("{free:.1} / {eff:.1} m²")));
            grid.set_property_read_only("gh_vent_free", true);
            grid.set_help_string("gh_vent_free", "The vents open at their design angle; effective is the free area times the discharge coefficient (0.6)");
            let shade = v["frame_shading"].as_f64().unwrap_or(0.0);
            grid.append(Property::string("Frame Shading", "gh_frame_shading", format!("{:.1} % of the roof", shade * 100.0)));
            grid.set_property_read_only("gh_frame_shading", true);
            let warnings: Vec<&str> = v["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if !warnings.is_empty() {
                grid.append(Property::string("Warnings", "gh_warnings", warnings.join("; ")));
                grid.set_property_read_only("gh_warnings", true);
                grid.set_help_string("gh_warnings", &warnings.join("\n"));
            }
        }
        if let Some((owner, _)) = self.enclosure_of_selection(id) {
            self.climate_rows(grid, owner);
        }
    }

    /// The enclosure's growing climate and crop light (ARC-GLZ-008, -009).
    fn climate_rows(&mut self, grid: &PropertyGrid, owner: ElementId) {
        let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store().clone()) else { return };
        let c = store.get(owner).and_then(reviera_core::hothouse::climate);
        grid.append(Property::category("Growing Climate", "c_climate"));
        let crops = reviera_core::hothouse::crops();
        let names: Vec<String> = std::iter::once("None".to_owned()).chain(crops.iter().map(|c| c.name.to_owned())).collect();
        let at = c.as_ref().and_then(|c| c.crop.as_deref()).and_then(|id| crops.iter().position(|k| k.id == id)).map_or(0, |i| i + 1) as i32;
        let at = if c.is_some() && at == 0 { names.len() as i32 } else { at };
        let mut choices: Vec<(String, i32)> = names.iter().enumerate().map(|(i, n)| (n.clone(), i as i32)).collect();
        if at == names.len() as i32 {
            choices.push(("Custom".to_owned(), names.len() as i32));
        }
        grid.append(Property::enumeration("Crop", "gc_crop", choices, at));
        grid.set_help_string("gc_crop", "A crop preset's light target and set points (growing.crops); None removes the climate");
        let Some(c) = c else { return };
        let temp = |t: Option<f64>| t.map_or(String::new(), |t| format!("{t} °C"));
        let rows = [
            ("Target DLI (mol/m²·d)", "gc_dli", format!("{}", c.target_dli)),
            ("Day Temperature", "gc_day", temp(c.day_temperature)),
            ("Night Temperature", "gc_night", temp(c.night_temperature)),
            ("CO₂ Set Point (ppm)", "gc_co2", format!("{}", c.co2)),
            ("Photoperiod (h)", "gc_photoperiod", format!("{}", c.photoperiod)),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        grid.set_help_string("gc_photoperiod", "Hours of light a day, lamps included (tomato needs 4–6 h of dark)");
        // Screens (ARC-GLZ-010) and the quick heat loss (ARC-GLZ-011).
        let kinds: Vec<String> = store.get(owner).and_then(reviera_core::enclosures::recipe).map(|r| r.screens.iter().map(|s| serde_json::to_value(s.kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()).collect()).unwrap_or_default();
        let at = SCREEN_SETS.iter().position(|(_, k)| k.iter().map(|x| x.to_string()).collect::<Vec<_>>() == kinds).unwrap_or(0) as i32;
        grid.append(Property::enumeration("Screens", "gc_screens", SCREEN_SETS.iter().enumerate().map(|(i, s)| (s.0.to_owned(), i as i32)).collect::<Vec<_>>(), at));
        grid.set_help_string("gc_screens", "Energy, shade and blackout screens under the gutters (enclosure.set_screens chooses their materials)");
        match self.run("enclosure.heat_loss", json!({ "enclosure_id": owner })) {
            Ok(h) => {
                let kw = h["total"].as_f64().unwrap_or(0.0) / 1000.0;
                let mut text = format!("{kw:.1} kW ({:.0} W/m²) at {:.0} K", h["per_area"].as_f64().unwrap_or(0.0), h["delta_t"].as_f64().unwrap_or(0.0));
                if let Some(s) = h["with_screen"].as_f64() {
                    text.push_str(&format!("; {:.1} kW with the energy screen closed", s / 1000.0));
                }
                grid.append(Property::string("Heat Loss", "gc_heat_loss", text));
                grid.set_help_string("gc_heat_loss", "The quick heat loss by the greenhouse table method: glazing U·A × construction and wind factors, plus air leakage, at the night set point against the weather's heating design temperature");
            }
            Err(e) => {
                // The reason only (without its error code): usually the weather to set.
                let why = e.split_once(": ").map_or(e.as_str(), |(_, m)| m).to_owned();
                grid.append(Property::string("Heat Loss", "gc_heat_loss", why));
            }
        }
        grid.set_property_read_only("gc_heat_loss", true);
        // Ventilation (ARC-GLZ-012) and CO₂ (ARC-GLZ-013) sizing.
        if let Ok(v) = self.run("enclosure.ventilation", json!({ "enclosure_id": owner })) {
            let text = format!("{:.1} m³/s: {} fan(s) of {:.0} m³/s, {:.1} m² of inlets", v["airflow"].as_f64().unwrap_or(0.0), v["fans"], v["fan_airflow"].as_f64().unwrap_or(0.0), v["inlet_area"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Fan Ventilation", "gc_fans", text));
            grid.set_help_string("gc_fans", "Summer fan airflow at 8 cfm/ft² of floor (one air change a minute), with inlets of at least 1.25 × the fans' face area; enclosure.ventilation sizes pads and HAF too");
            let text = format!("roof {:.0} %, sides {:.0} % of the floor (15–20 % each)", v["roof_vent_percent"].as_f64().unwrap_or(0.0), v["side_vent_percent"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Natural Vents", "gc_vents", text));
            grid.set_property_read_only("gc_fans", true);
            grid.set_property_read_only("gc_vents", true);
        }
        if let Ok(c) = self.run("enclosure.co2", json!({ "enclosure_id": owner })) {
            let text = format!("{:.1} kg/h at {:.0} ppm (burner {:.0}–{:.0} kW)", c["supply"].as_f64().unwrap_or(0.0), c["setpoint"].as_f64().unwrap_or(0.0), c["burner_kw"][0].as_f64().unwrap_or(0.0), c["burner_kw"][1].as_f64().unwrap_or(0.0));
            grid.append(Property::string("CO₂ Supply", "gc_co2_supply", text));
            grid.set_property_read_only("gc_co2_supply", true);
            grid.set_help_string("gc_co2_supply", "To hold the set point with the vents shut; dose from half an hour after sunrise, cut back once the vents open more than 10 %");
        }
        match self.run("enclosure.crop_light", json!({ "enclosure_id": owner })) {
            Ok(l) => {
                let m = &l["months"][l["darkest_month"].as_u64().unwrap_or(1) as usize - 1];
                const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
                let name = MONTHS.get(m["month"].as_u64().unwrap_or(1) as usize - 1).copied().unwrap_or("");
                let text = format!("{name}: {:.1} natural + {:.1} lamps = {:.1} mol/m²·d", m["natural"].as_f64().unwrap_or(0.0), m["lamps"].as_f64().unwrap_or(0.0), m["total"].as_f64().unwrap_or(0.0));
                grid.append(Property::string("Darkest Month", "gc_darkest", text));
                grid.set_property_read_only("gc_darkest", true);
                let t = l["transmission"]["total"].as_f64().unwrap_or(0.0);
                grid.append(Property::string("Roof Transmission", "gc_transmission", format!("{:.0} % (glazing and bars)", t * 100.0)));
                grid.set_property_read_only("gc_transmission", true);
                let need = l["required_ppfd"].as_f64().unwrap_or(0.0);
                let text = format!("{need:.0} µmol/m²·s needed, {:.0} laid out", l["lamp_ppfd"].as_f64().unwrap_or(0.0));
                grid.append(Property::string("Lamp PPFD", "gc_ppfd", text));
                grid.set_property_read_only("gc_ppfd", true);
                if let Some(closed) = l["months"].as_array().map(|a| a.iter().filter(|m| m["shade_closed"] == true).count()).filter(|_| l["months"][0].get("shaded").is_some()) {
                    grid.append(Property::string("Shade Screen Closed", "gc_shade", format!("{closed} month(s) (natural light 30 % over the target)")));
                    grid.set_property_read_only("gc_shade", true);
                }
                let short = l["months"].as_array().map_or(0, |a| a.iter().filter(|m| m["meets_target"] == false).count());
                grid.append(Property::string("Months Short of Target", "gc_short", short.to_string()));
                grid.set_property_read_only("gc_short", true);
                let src = if l["source"] == "weather" { "the weather file" } else { "an estimate from the latitude (clearness 0.5)" };
                grid.set_help_string("gc_darkest", &format!("Outdoor light from {src}, through the roof glazing and past its bars"));
            }
            Err(e) => {
                grid.append(Property::string("Crop Light", "gc_darkest", e.to_string()));
                grid.set_property_read_only("gc_darkest", true);
            }
        }
    }

    /// An edit of a growing climate row; `false` if `property` is not one.
    pub(crate) fn climate_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("gc_") {
            return false;
        }
        let frame = self.frame;
        let Some((owner, _)) = self.selection().first().and_then(|id| self.enclosure_of_selection(*id)) else { return true };
        let text = text.unwrap_or_default();
        let mut args = json!({ "enclosure_id": owner });
        let number = |t: &str| t.trim().trim_end_matches("°C").trim().parse::<f64>().ok();
        match property {
            "gc_screens" => {
                let Some((_, kinds)) = SCREEN_SETS.iter().find(|s| s.0 == text) else { return true };
                let screens: Vec<_> = kinds.iter().map(|k| json!({ "kind": k })).collect();
                if let Err(e) = self.run("enclosure.set_screens", json!({ "enclosure_id": owner, "screens": screens })) {
                    error(&frame, &format!("The screens were not changed:\n{e}"));
                }
                self.selection_changed();
                self.refresh_scene(&frame);
                return true;
            }
            "gc_crop" if text == "None" => args["clear"] = json!(true),
            "gc_crop" => match reviera_core::hothouse::crops().iter().find(|c| c.name == text) {
                Some(c) => args["crop"] = json!(c.id),
                None => return true,
            },
            "gc_dli" | "gc_day" | "gc_night" | "gc_co2" | "gc_photoperiod" => {
                let key = match property {
                    "gc_dli" => "target_dli",
                    "gc_day" => "day_temperature",
                    "gc_night" => "night_temperature",
                    "gc_co2" => "co2",
                    _ => "photoperiod",
                };
                match number(&text) {
                    Some(v) => args[key] = json!(v),
                    None => return true,
                }
            }
            _ => return true,
        }
        if let Err(e) = self.run("enclosure.set_climate", args) {
            error(&frame, &format!("The climate was not changed:\n{e}"));
        }
        self.selection_changed();
        true
    }

    /// An edit of an enclosure row; `false` if `property` is not one.
    pub(crate) fn enclosure_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("gh_") {
            return false;
        }
        let frame = self.frame;
        let Some((owner, _)) = self.selection().first().and_then(|id| self.enclosure_of_selection(*id)) else { return true };
        let text = text.unwrap_or_default();
        let mut args = json!({ "enclosure_id": owner });
        match property {
            "gh_length" => args["length"] = json!(text),
            "gh_span" => args["span"] = json!(text),
            "gh_spans" => match text.trim().parse::<u32>() {
                Ok(n) => args["spans"] = json!(n),
                Err(_) => return true,
            },
            "gh_bay" => args["bay"] = json!(text),
            "gh_eave" => args["eave"] = json!(text),
            "gh_pitch" => args["pitch"] = json!(text),
            "gh_knee" => args["knee"] = json!(text),
            "gh_gutters" => args["gutters"] = json!(matches!(text.as_str(), "True" | "true" | "1")),
            "gh_roof_glazing" | "gh_wall_glazing" => {
                let key = if property == "gh_roof_glazing" { "roof_glazing" } else { "wall_glazing" };
                args[key] = json!(if text == "Greenhouse default" { String::new() } else { text });
            }
            "gh_side_vents" => match text.trim().trim_end_matches('%').trim().parse::<f64>() {
                Ok(v) => args["side_vents"] = json!(v),
                Err(_) => return true,
            },
            "gh_vent_angle" => args["vent_angle"] = json!(text),
            "gh_roll_up" => args["roll_up"] = json!(text),
            "gh_headhouse" => args["headhouse"] = json!({ "enabled": matches!(text.as_str(), "True" | "true" | "1") }),
            "gh_hh_depth" => args["headhouse"] = json!({ "depth": text }),
            "gh_hh_height" => args["headhouse"] = json!({ "height": text }),
            "gh_hh_at" => args["headhouse"] = json!({ "at": if text == "End" { "end" } else { "start" } }),
            "gh_gable_vents" => args["gable_vents"] = json!(text),
            "gh_vent_actuator" => match ACTUATORS.iter().find(|a| a.1 == text) {
                Some(a) => args["vent_actuator"] = json!(a.0),
                None => return true,
            },
            "gh_frame_system" => args["frame_system"] = json!(if text == "Greenhouse default" { String::new() } else { text }),
            "gh_doors" => args["doors"] = json!(matches!(text.as_str(), "True" | "true" | "1")),
            "gh_frame" => args["frame"] = json!(matches!(text.as_str(), "True" | "true" | "1")),
            "gh_vents" => match text.trim().trim_end_matches('%').trim().parse::<f64>() {
                Ok(v) => args["vents"] = json!(v),
                Err(_) => return true,
            },
            _ => return true,
        }
        if let Err(e) = self.run("enclosure.set", args) {
            error(&frame, &format!("The enclosure was not changed:\n{e}"));
        }
        // The selected part may have been rebuilt: select the enclosure.
        let _ = self.run("selection.set", json!({ "ids": [owner] }));
        self.selection_changed();
        self.refresh_scene(&frame);
        true
    }
}

const SYSTEMS: [(reviera_core::growing::System, &str); 5] = [
    (reviera_core::growing::System::Benches, "Benches"),
    (reviera_core::growing::System::RollingBenches, "Rolling benches"),
    (reviera_core::growing::System::EbbFlow, "Ebb-and-flow tables"),
    (reviera_core::growing::System::Nft, "NFT tables"),
    (reviera_core::growing::System::HighWire, "High-wire gutters"),
];

impl Gui {
    /// Architecture › Growing Layout: fill the selected enclosure (or room) with a growing system.
    pub(crate) fn growing_dialog(&mut self) {
        let frame = self.frame;
        let room = |g: &Gui, id: ElementId| {
            g.doc.and_then(|d| g.app.document(d)).and_then(|d| d.doc.store().get(id)).and_then(reviera_core::rooms::room_recipe).map(|_| id)
        };
        let Some(owner) = self.selection().first().and_then(|id| self.enclosure_of_selection(*id).map(|(o, _)| o).or_else(|| room(self, *id))) else {
            return frame.set_status_text("Growing Layout: select a glazed enclosure or a room first", 0);
        };
        let dialog = Dialog::builder(&frame, "Growing Layout").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        grid.add_growable_col(1, 1);
        let choice = |label: &str, items: Vec<String>, at: u32| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let c = Choice::builder(&dialog).with_choices(items).build();
            c.set_selection(at);
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&c, 1, SizerFlag::Expand, 0);
            c
        };
        let system = choice("System", SYSTEMS.iter().map(|s| s.1.to_owned()).collect(), 0);
        let direction = choice("Runs", vec!["Across the span, main aisle along the middle".into(), "Along the length, main aisle across the middle".into()], 0);
        let field = |label: &str| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let t = TextCtrl::builder(&dialog).with_value("").build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&t, 1, SizerFlag::Expand, 0);
            t
        };
        let width = field("Width (blank: the system's)");
        let aisle = field("Aisle");
        let main = field("Main aisle");
        let height = field("Growing surface height");
        let wire = field("High-wire: crop wire height");
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        dialog.set_escape_id(ID_CANCEL);
        let code = dialog.show_modal();
        let mut args = json!({
            "enclosure_id": owner,
            "system": system.get_selection().and_then(|i| SYSTEMS.get(i as usize)).map(|s| s.0),
            "direction": if direction.get_selection() == Some(1) { "along" } else { "across" },
        });
        let mut bad = false;
        for (key, t) in [("width", &width), ("aisle", &aisle), ("main_aisle", &main), ("height", &height), ("wire", &wire)] {
            let v = t.get_value();
            if v.trim().is_empty() {
                continue;
            }
            match self.run("units.parse", json!({ "text": v, "data_type": "length" })) {
                Ok(m) => args[key] = m,
                Err(_) => bad = true,
            }
        }
        dialog.destroy();
        if code != ID_OK {
            return;
        }
        if bad {
            return error(&frame, "Growing Layout: the sizes are lengths");
        }
        match self.run("growing.layout", args) {
            Ok(v) => {
                let s = self.run("growing.summary", json!({ "layout_id": v })).unwrap_or_default();
                frame.set_status_text(
                    &format!("Growing layout: {} pieces, {:.0} m² growing ({:.0} % of the floor)", s["pieces"], s["growing_area"].as_f64().unwrap_or(0.0), s["share"].as_f64().unwrap_or(0.0)),
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The layout was not made:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// The layout of a selected layout or piece.
    fn layout_of_selection(&self, id: ElementId) -> Option<(ElementId, reviera_core::growing::LayoutRecipe)> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(id)?;
        let owner = match e.param(reviera_core::growing::PART_KEY).and_then(|v| v.as_str()) {
            Some(g) => store.find_by_guid(&reviera_core::GlobalId::parse(g)?)?,
            None => id,
        };
        Some((owner, reviera_core::growing::recipe(store.get(owner)?)?))
    }

    /// A growing layout's rows, for a selected layout or piece.
    pub(crate) fn growing_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some((owner, r)) = self.layout_of_selection(id) else { return };
        grid.append(Property::category("Growing Layout", "c_growing"));
        let index = SYSTEMS.iter().position(|s| s.0 == r.system).map_or(-1, |i| i as i32);
        grid.append(Property::enumeration("System", "gl_system", SYSTEMS.iter().enumerate().map(|(i, s)| (s.1.to_owned(), i as i32)).collect::<Vec<_>>(), index));
        let dir = if r.direction == reviera_core::growing::Direction::Along { 1 } else { 0 };
        grid.append(Property::enumeration("Runs", "gl_direction", vec![("Across".to_owned(), 0), ("Along".to_owned(), 1)], dir));
        for (label, key, v) in [("Width", "gl_width", r.width), ("Aisle", "gl_aisle", r.aisle), ("Main Aisle", "gl_main", r.main_aisle), ("Height", "gl_height", r.height)] {
            let text = self.format_length(v);
            grid.append(Property::string(label, key, text));
        }
        if r.system == reviera_core::growing::System::HighWire {
            let text = self.format_length(r.wire);
            grid.append(Property::string("Crop Wire", "gl_wire", text));
            grid.set_help_string("gl_wire", "The crop wire's height above the floor (kept under the eaves or the room's top)");
        }
        if let Ok(s) = self.run("growing.summary", json!({ "layout_id": owner })) {
            let text = format!("{} pieces, {:.1} m² ({:.0} % of the floor)", s["pieces"], s["growing_area"].as_f64().unwrap_or(0.0), s["share"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Growing Area", "gl_area", text));
            grid.set_property_read_only("gl_area", true);
        }
    }

    /// An edit of a growing layout row; `false` if `property` is not one.
    pub(crate) fn growing_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("gl_") {
            return false;
        }
        let frame = self.frame;
        let Some((owner, _)) = self.selection().first().and_then(|id| self.layout_of_selection(*id)) else { return true };
        let text = text.unwrap_or_default();
        let mut args = json!({ "layout_id": owner });
        let length = |g: &mut Gui, t: &str| g.run("units.parse", json!({ "text": t, "data_type": "length" })).ok();
        match property {
            "gl_system" => match SYSTEMS.iter().find(|s| s.1 == text) {
                Some(s) => args["system"] = json!(s.0),
                None => return true,
            },
            "gl_direction" => args["direction"] = json!(if text == "Along" { "along" } else { "across" }),
            "gl_width" | "gl_aisle" | "gl_main" | "gl_height" | "gl_wire" => {
                let key = match property {
                    "gl_width" => "width",
                    "gl_aisle" => "aisle",
                    "gl_main" => "main_aisle",
                    "gl_wire" => "wire",
                    _ => "height",
                };
                match length(self, &text) {
                    Some(v) => args[key] = v,
                    None => return true,
                }
            }
            _ => return true,
        }
        if let Err(e) = self.run("growing.set_layout", args) {
            error(&frame, &format!("The layout was not changed:\n{e}"));
        }
        let _ = self.run("selection.set", json!({ "ids": [owner] }));
        self.selection_changed();
        self.refresh_scene(&frame);
        true
    }
}

impl Gui {
    /// Architecture › Grow Lights: top-lights in the selected enclosure, for its crop's darkest
    /// month (ARC-GLZ-009).
    pub(crate) fn grow_lights(&mut self) {
        let frame = self.frame;
        let Some(owner) = self.selection().first().and_then(|id| self.enclosure_of_selection(*id).map(|(o, _)| o)) else {
            return frame.set_status_text("Grow Lights: select a glazed enclosure first", 0);
        };
        match self.run("growing.lights", json!({ "enclosure_id": owner })) {
            Ok(v) => {
                let s = self.run("growing.lights_summary", json!({ "lights_id": v })).unwrap_or_default();
                frame.set_status_text(
                    &format!("Grow lights: {} fixtures, {:.0} µmol/m²·s, {:.0} W/m²", s["fixtures"], s["average_ppfd"].as_f64().unwrap_or(0.0), s["power_density"].as_f64().unwrap_or(0.0)),
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The grow lights were not laid out:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// The grow-light layout of a selected layout or fixture.
    fn lights_of_selection(&self, id: ElementId) -> Option<(ElementId, reviera_core::hothouse::LightsRecipe)> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(id)?;
        let owner = match e.param(reviera_core::hothouse::LIGHTS_KEY).and_then(|v| v.as_str()) {
            Some(g) => store.find_by_guid(&reviera_core::GlobalId::parse(g)?)?,
            None => id,
        };
        Some((owner, reviera_core::hothouse::lights_recipe(store.get(owner)?)?))
    }

    /// A grow-light layout's rows, for a selected layout or fixture.
    pub(crate) fn grow_light_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some((owner, r)) = self.lights_of_selection(id) else { return };
        grid.append(Property::category("Grow Lights", "c_grow_lights"));
        let rows = [
            ("Photon Flux (µmol/s)", "lt_flux", format!("{}", r.photon_flux)),
            ("Power (W)", "lt_power", format!("{}", r.power)),
            ("Target PPFD (µmol/m²·s)", "lt_target", format!("{:.0}", r.target_ppfd)),
            ("Maintenance Factor", "lt_maintenance", format!("{}", r.maintenance)),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        let h = self.format_length(r.height);
        grid.append(Property::string("Height", "lt_height", h));
        grid.set_help_string("lt_height", "The fixtures' underside above the floor (kept under the gutters)");
        if let Ok(s) = self.run("growing.lights_summary", json!({ "lights_id": owner })) {
            let text = format!("{} fixtures, {:.0} µmol/m²·s, {:.1} mol/m²·d", s["fixtures"], s["average_ppfd"].as_f64().unwrap_or(0.0), s["lamp_dli"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Layout", "lt_summary", text));
            grid.set_property_read_only("lt_summary", true);
            let text = format!("{:.1} kW, {:.0} W/m², {:.2} µmol/J", s["power"].as_f64().unwrap_or(0.0) / 1000.0, s["power_density"].as_f64().unwrap_or(0.0), s["efficacy"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Power", "lt_load", text));
            grid.set_property_read_only("lt_load", true);
            let warnings: Vec<&str> = s["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if !warnings.is_empty() {
                grid.append(Property::string("Warnings", "lt_warnings", warnings.join("; ")));
                grid.set_property_read_only("lt_warnings", true);
            }
        }
    }

    /// An edit of a grow-light row; `false` if `property` is not one.
    pub(crate) fn grow_light_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("lt_") {
            return false;
        }
        let frame = self.frame;
        let Some((owner, _)) = self.selection().first().and_then(|id| self.lights_of_selection(*id)) else { return true };
        let text = text.unwrap_or_default();
        let mut args = json!({ "lights_id": owner });
        match property {
            "lt_height" => match self.run("units.parse", json!({ "text": text, "data_type": "length" })) {
                Ok(v) => args["height"] = v,
                Err(_) => return true,
            },
            "lt_flux" | "lt_power" | "lt_target" | "lt_maintenance" => {
                let key = match property {
                    "lt_flux" => "photon_flux",
                    "lt_power" => "power",
                    "lt_target" => "target_ppfd",
                    _ => "maintenance",
                };
                match text.trim().parse::<f64>() {
                    Ok(v) => args[key] = json!(v),
                    Err(_) => return true,
                }
            }
            _ => return true,
        }
        if let Err(e) = self.run("growing.set_lights", args) {
            error(&frame, &format!("The grow lights were not changed:\n{e}"));
        }
        let _ = self.run("selection.set", json!({ "ids": [owner] }));
        self.selection_changed();
        self.refresh_scene(&frame);
        true
    }
}
