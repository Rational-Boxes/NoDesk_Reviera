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

const FORMS: [(Form, &str); 5] = [
    (Form::Gable, "Gable"),
    (Form::Multispan, "Gutter-connected multispan"),
    (Form::Venlo, "Venlo"),
    (Form::Hoop, "Hoop tunnel"),
    (Form::Gothic, "Gothic tunnel"),
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
        grid.append(Property::boolean("Gutters", "gh_gutters", r.gutters));
        grid.append(Property::boolean("Doors", "gh_doors", r.doors));
        grid.append(Property::boolean("Frame", "gh_frame", r.frame));
        grid.set_help_string("gh_frame", "Steel posts at every bay on each gutter line");
        grid.append(Property::string("Ridge Vents (% of floor)", "gh_vents", format!("{}", r.vents)));
        grid.set_help_string("gh_vents", "The ridge vents' target area as a share of the floor area; 0 for none");
        grid.set_help_string("gh_doors", "A door in the middle of each end wall");
        let ridge = self.format_length(r.ridge());
        grid.append(Property::string("Ridge Height", "gh_ridge", ridge));
        grid.set_property_read_only("gh_ridge", true);
        let area = r.width() * r.length;
        grid.append(Property::string("Floor Area", "gh_area", format!("{area:.1} m²")));
        grid.set_property_read_only("gh_area", true);
        let vents = self.enclosure_of_selection(id).and_then(|(owner, _)| self.run("enclosure.get", json!({ "enclosure_id": owner })).ok());
        if let Some(v) = vents {
            let (a, pc) = (v["vent_area"].as_f64().unwrap_or(0.0), v["vent_percent"].as_f64().unwrap_or(0.0));
            grid.append(Property::string("Ridge Vent Area", "gh_vent_area", format!("{a:.1} m² ({pc:.1} %)")));
            grid.set_property_read_only("gh_vent_area", true);
        }
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

const SYSTEMS: [(reviera_core::growing::System, &str); 4] = [
    (reviera_core::growing::System::Benches, "Benches"),
    (reviera_core::growing::System::RollingBenches, "Rolling benches"),
    (reviera_core::growing::System::EbbFlow, "Ebb-and-flow tables"),
    (reviera_core::growing::System::Nft, "NFT tables"),
];

impl Gui {
    /// Architecture › Growing Layout: fill the selected enclosure with a growing system.
    pub(crate) fn growing_dialog(&mut self) {
        let frame = self.frame;
        let Some((owner, _)) = self.selection().first().and_then(|id| self.enclosure_of_selection(*id)) else {
            return frame.set_status_text("Growing Layout: select a glazed enclosure first", 0);
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
        for (key, t) in [("width", &width), ("aisle", &aisle), ("main_aisle", &main), ("height", &height)] {
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
            "gl_width" | "gl_aisle" | "gl_main" | "gl_height" => {
                let key = match property {
                    "gl_width" => "width",
                    "gl_aisle" => "aisle",
                    "gl_main" => "main_aisle",
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
