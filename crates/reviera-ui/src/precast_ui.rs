// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Precast detailing in the window (STR-PRE-001, STR-PRE-002): the Configuration dialog (a tree
//! of pages; a value breaking a rule marks its field and page and disables OK), and Split /
//! Re-split on walls and floors.

use std::rc::Rc;

use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// How a field is shown: metres as millimetres, a plain number, text or a check box.
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Mm,
    Number,
    Text,
    Bool,
    /// Reinforcement types, one per line (STR-PRE-003).
    AreaTypes,
    EdgeTypes,
    /// Custom fabric sheet rules, one per line (STR-PRE-004).
    FabricRules,
}

/// The fields of a page: path, label, unit.
type Fields = &'static [(&'static str, &'static str, Unit)];

const WALL: Fields = &[
    ("segmentation.min_length", "Minimum length (mm)", Unit::Mm),
    ("segmentation.max_length", "Maximum length (mm)", Unit::Mm),
    ("segmentation.min_height", "Minimum height (mm)", Unit::Mm),
    ("segmentation.max_height", "Maximum height (mm)", Unit::Mm),
    ("segmentation.max_weight", "Maximum weight (t)", Unit::Number),
    ("segmentation.joint_gap_vertical", "Vertical joint gap (mm)", Unit::Mm),
    ("segmentation.joint_gap_horizontal", "Horizontal joint gap (mm)", Unit::Mm),
    ("segmentation.min_opening_distance", "Minimum distance to openings (mm)", Unit::Mm),
    ("segmentation.profile_overlap", "Profile overlap height (mm)", Unit::Mm),
];

const SLAB: Fields = &[
    ("segmentation.min_width", "Minimum width (mm)", Unit::Mm),
    ("segmentation.max_width", "Maximum width (mm)", Unit::Mm),
    ("segmentation.max_weight", "Maximum weight (t)", Unit::Number),
    ("segmentation.joint_gap", "Joint gap (mm)", Unit::Mm),
    ("segmentation.family", "Hollow-core component", Unit::Text),
    ("segmentation.division_profile", "Division profile", Unit::Text),
    ("segmentation.strand_diameter", "Strand diameter (mm)", Unit::Mm),
];

const GENERAL: Fields = &[
    ("density", "Concrete density (t/m³)", Unit::Number),
    ("auto_update", "Automatic updates", Unit::Bool),
    ("data_folder", "Data folder", Unit::Text),
];

const WALL_REBAR: Fields = &[
    ("reinforcement.area_types", "Area types", Unit::AreaTypes),
    ("reinforcement.edge_types", "Edge U-bar types", Unit::EdgeTypes),
];

const SLAB_REBAR: Fields = &[("reinforcement.area_types", "Area types", Unit::AreaTypes)];

const FABRIC: Fields = &[("fabric_rules", "Rules", Unit::FabricRules)];

const CAM: Fields = &[("cam.format", "Format", Unit::Text), ("cam.folder", "Output folder", Unit::Text), ("cam.filename", "Filename rule", Unit::Text)];

enum Ctl {
    Text(TextCtrl),
    Check(CheckBox),
}

struct Field {
    path: String,
    unit: Unit,
    ctl: Ctl,
    page: usize,
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

fn at_mut<'a>(v: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    path.split('.').try_fold(v, |v, k| v.get_mut(k))
}

fn shown(v: Option<&Value>, unit: Unit) -> String {
    let mm = |v: &Value| format!("{}", (v.as_f64().unwrap_or(0.0) * 1e6).round() / 1000.0);
    match (v, unit) {
        (Some(Value::Array(list)), Unit::FabricRules) => list
            .iter()
            .map(|t| {
                let mut c = vec![t["id"].as_str().unwrap_or("").to_owned(), t["major_wire"].as_str().unwrap_or("").to_owned(), t["minor_wire"].as_str().unwrap_or("").to_owned()];
                for k in ["major_spacing", "minor_spacing", "edge_outer", "edge_joint", "opening_clearance", "min_wire_length", "max_length", "max_width", "lap"] {
                    c.push(mm(&t[k]));
                }
                c.push(t["layer"].as_str().unwrap_or("both").to_owned());
                c.push(t["name"].as_str().unwrap_or("").to_owned());
                c.join("; ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        (Some(Value::Array(list)), Unit::AreaTypes | Unit::EdgeTypes) => list
            .iter()
            .map(|t| {
                let mut cells = vec![
                    t["id"].as_str().unwrap_or("").to_owned(),
                    format!("{}-{}", mm(&t["min_thickness"]), mm(&t["max_thickness"])),
                    t["bar_type"].as_str().unwrap_or("").to_owned(),
                    mm(&t["spacing"]),
                ];
                if unit == Unit::EdgeTypes {
                    cells.push(mm(&t["leg"]));
                }
                cells.push(t["name"].as_str().unwrap_or("").to_owned());
                cells.join("; ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        (Some(Value::Number(n)), Unit::Mm) => format!("{}", (n.as_f64().unwrap_or(0.0) * 1e6).round() / 1000.0),
        (Some(Value::Number(n)), _) => format!("{}", n.as_f64().unwrap_or(0.0)),
        (Some(Value::String(s)), _) => s.clone(),
        _ => String::new(),
    }
}

/// Reinforcement types from lines `id; min-max mm; bar type; spacing mm; [leg mm;] name`.
fn types(text: &str, edge: bool) -> Option<Value> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let c: Vec<&str> = line.split(';').map(str::trim).collect();
        let need = if edge { 5 } else { 4 };
        if c.len() < need {
            return None;
        }
        let (lo, hi) = c[1].split_once('-')?;
        let mm = |s: &str| s.trim().parse::<f64>().ok().map(|v| v / 1000.0);
        let mut t = json!({
            "id": c[0], "min_thickness": mm(lo)?, "max_thickness": mm(hi)?, "bar_type": c[2], "spacing": mm(c[3])?,
            "name": c.get(need).copied().unwrap_or(c[0]),
        });
        if edge {
            t["leg"] = json!(mm(c[4])?);
        }
        out.push(t);
    }
    Some(Value::Array(out))
}

/// Custom fabric rules from lines `id; major wire; minor wire; major spacing; minor spacing;
/// outer edge; joint edge; opening clearance; shortest wire; largest length; largest width; lap
/// (mm); layer; name`.
fn fabric_rules(text: &str) -> Option<Value> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let c: Vec<&str> = line.split(';').map(str::trim).collect();
        if c.len() < 13 {
            return None;
        }
        let mm = |s: &str| s.parse::<f64>().ok().map(|v| v / 1000.0);
        let keys = ["major_spacing", "minor_spacing", "edge_outer", "edge_joint", "opening_clearance", "min_wire_length", "max_length", "max_width", "lap"];
        let mut r = json!({ "id": c[0], "major_wire": c[1], "minor_wire": c[2], "layer": c[12], "name": c.get(13).copied().unwrap_or(c[0]) });
        for (k, v) in keys.iter().zip(&c[3..12]) {
            r[*k] = json!(mm(v)?);
        }
        out.push(r);
    }
    Some(Value::Array(out))
}

/// The dialog's values over `base`; the paths whose text is not a number.
fn read(base: &Value, fields: &[Field]) -> (Value, Vec<String>) {
    let mut v = base.clone();
    let mut bad = Vec::new();
    for f in fields {
        let value = match (&f.ctl, f.unit) {
            (Ctl::Check(c), _) => json!(c.get_value()),
            (Ctl::Text(t), Unit::Text) => json!(t.get_value()),
            (Ctl::Text(t), Unit::FabricRules) => match fabric_rules(&t.get_value()) {
                Some(list) => list,
                None => {
                    bad.push(f.path.clone());
                    continue;
                }
            },
            (Ctl::Text(t), u @ (Unit::AreaTypes | Unit::EdgeTypes)) => match types(&t.get_value(), u == Unit::EdgeTypes) {
                Some(list) => list,
                None => {
                    bad.push(f.path.clone());
                    continue;
                }
            },
            (Ctl::Text(t), u) => match t.get_value().trim().parse::<f64>() {
                Ok(x) => json!(if u == Unit::Mm { x / 1000.0 } else { x }),
                Err(_) => {
                    bad.push(f.path.clone());
                    continue;
                }
            },
        };
        if let Some(slot) = at_mut(&mut v, &f.path) {
            *slot = value;
        }
    }
    (v, bad)
}

impl Gui {
    /// Concrete › Precast Settings › Configuration (STR-PRE-001).
    pub(crate) fn precast_configuration(&mut self) {
        let frame = self.frame;
        let Ok(base) = self.run("precast_config.get", json!({ "path": "" })) else { return };
        let affected = self.run("precast_config.affected", json!({})).unwrap_or_default();
        let older: Vec<u64> = affected["older"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let dialog = Dialog::builder(&frame, "Precast Configuration").with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let book = Treebook::builder(&dialog).build();
        let mut fields: Vec<Field> = Vec::new();
        let mut pages: Vec<String> = Vec::new();
        let page = |label: &str, prefix: &str, list: Fields, sub: bool, fields: &mut Vec<Field>, pages: &mut Vec<String>| {
            let panel = Panel::builder(&book).build();
            let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
            grid.add_growable_col(1, 1);
            let index = pages.len();
            for (path, name, unit) in list {
                let full = if prefix.is_empty() { (*path).to_owned() } else { format!("{prefix}.{path}") };
                let l = StaticText::builder(&panel).with_label(name).build();
                grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
                let ctl = if *unit == Unit::Bool {
                    let c = CheckBox::builder(&panel).with_label("").build();
                    c.set_value(at(&base, &full).and_then(Value::as_bool).unwrap_or(false));
                    grid.add(&c, 1, SizerFlag::Expand, 0);
                    Ctl::Check(c)
                } else if *unit == Unit::FabricRules {
                    let t = TextCtrl::builder(&panel).with_value(&shown(at(&base, &full), *unit)).with_style(TextCtrlStyle::MultiLine).build();
                    t.set_min_size(Size::new(560, 120));
                    t.set_tooltip("One rule per line: id; main wire; cross wire; main spacing; cross spacing; outer edge; joint edge; opening clearance; shortest wire; largest length; largest width; lap (mm); layer (outer, inner, both); name");
                    grid.add(&t, 1, SizerFlag::Expand, 0);
                    Ctl::Text(t)
                } else if matches!(unit, Unit::AreaTypes | Unit::EdgeTypes) {
                    let t = TextCtrl::builder(&panel).with_value(&shown(at(&base, &full), *unit)).with_style(TextCtrlStyle::MultiLine).build();
                    t.set_min_size(Size::new(420, 110));
                    t.set_tooltip(if *unit == Unit::EdgeTypes {
                        "One type per line: id; thickness from-to (mm); bar type; spacing (mm); leg (mm); name"
                    } else {
                        "One type per line: id; thickness from-to (mm); bar type; spacing (mm); name"
                    });
                    grid.add(&t, 1, SizerFlag::Expand, 0);
                    Ctl::Text(t)
                } else {
                    let t = TextCtrl::builder(&panel).with_value(&shown(at(&base, &full), *unit)).build();
                    t.set_min_size(Size::new(220, -1));
                    grid.add(&t, 1, SizerFlag::Expand, 0);
                    Ctl::Text(t)
                };
                fields.push(Field { path: full, unit: *unit, ctl, page: index });
            }
            if list.is_empty() {
                let note = if sub || !prefix.is_empty() {
                    "Filled by later precast steps (Auto Reinforce, mounting parts, shop drawings): not built yet"
                } else {
                    "This kind's rules are in the pages under it: Segmentation, Reinforcement, Connections and Shop Drawing"
                };
                let l = StaticText::builder(&panel).with_label(note).build();
                grid.add(&l, 0, SizerFlag::All, 0);
            }
            let s = BoxSizer::builder(Orientation::Vertical).build();
            s.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
            panel.set_sizer(s, true);
            if sub {
                book.add_sub_page(&panel, label, false, -1);
            } else {
                book.add_page(&panel, label, index == 0, -1);
            }
            pages.push(label.to_owned());
        };
        page("General", "", GENERAL, false, &mut fields, &mut pages);
        for (label, prefix, list) in [
            ("Solid Wall", "wall.solid", WALL),
            ("Double Wall", "wall.double", WALL),
            ("Solid Slab", "slab.solid", SLAB),
            ("Hollow-core Slab", "slab.hollow_core", SLAB),
            ("Lattice-girder Slab", "slab.lattice_girder", SLAB),
        ] {
            page(label, "", &[], false, &mut fields, &mut pages);
            page("Segmentation", prefix, list, true, &mut fields, &mut pages);
            page("Reinforcement", prefix, if prefix.starts_with("wall") { WALL_REBAR } else { SLAB_REBAR }, true, &mut fields, &mut pages);
            for node in ["Connections", "Shop Drawing"] {
                page(node, "", &[], true, &mut fields, &mut pages);
            }
        }
        page("Custom Fabric Sheet Rules", "", FABRIC, false, &mut fields, &mut pages);
        page("Annotations", "-", &[], false, &mut fields, &mut pages);
        page("Dimensioning", "-", &[], false, &mut fields, &mut pages);
        page("CAM Export", "", CAM, false, &mut fields, &mut pages);
        sizer.add(&book, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let status = StaticText::builder(&dialog).with_label("").build();
        sizer.add(&status, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, 12);
        let rerun = CheckBox::builder(&dialog)
            .with_label(&format!("Re-run on the {} assemblies made with an older configuration", older.len()))
            .build();
        rerun.set_value(false);
        if older.is_empty() {
            rerun.enable(false);
        }
        sizer.add(&rerun, 0, SizerFlag::All, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let export = Button::builder(&dialog).with_label("Export…").build();
        let import = Button::builder(&dialog).with_label("Import…").build();
        buttons.add(&export, 0, SizerFlag::All, 6);
        buttons.add(&import, 0, SizerFlag::All, 6);
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let fields = Rc::new(fields);
        let normal = fields.iter().find_map(|f| if let Ctl::Text(t) = &f.ctl { Some(t.get_background_color()) } else { None }).unwrap_or(Colour::new(255, 255, 255, 255));
        // A red that keeps the theme's text readable.
        let dark = u32::from(normal.r) + u32::from(normal.g) + u32::from(normal.b) < 384;
        let marked = if dark { Colour::new(140, 35, 35, 255) } else { Colour::new(255, 205, 205, 255) };
        let check = {
            let (fields, base, pages) = (fields.clone(), base.clone(), pages.clone());
            move || {
                let (v, unreadable) = read(&base, &fields);
                let mut wrong: Vec<(String, String)> = unreadable.into_iter().map(|p| (p, "not a number".to_owned())).collect();
                if wrong.is_empty() {
                    match serde_json::from_value::<reviera_core::precast::Config>(v) {
                        Ok(c) => wrong.extend(reviera_core::precast::problems(&c)),
                        Err(e) => wrong.push((String::new(), e.to_string())),
                    }
                }
                let mut bad_pages = vec![false; pages.len()];
                for f in fields.iter() {
                    let is_bad = wrong.iter().any(|(p, _)| *p == f.path || p.starts_with(&format!("{}.", f.path)));
                    if is_bad {
                        bad_pages[f.page] = true;
                    }
                    if let Ctl::Text(t) = &f.ctl {
                        // Marked, else the theme's own colour.
                        t.set_background_color(if is_bad { marked } else { normal });
                        t.refresh(true, None);
                    }
                }
                for (i, name) in pages.iter().enumerate() {
                    let want = if bad_pages[i] { format!("{name} (!)") } else { name.clone() };
                    if book.get_page_text(i) != want {
                        book.set_page_text(i, &want);
                    }
                }
                ok.enable(wrong.is_empty());
                status.set_label(&wrong.first().map_or(String::new(), |(p, m)| if p.is_empty() { m.clone() } else { format!("{p}: {m}") }));
            }
        };
        let check = Rc::new(check);
        check();
        for f in fields.iter() {
            let c = check.clone();
            match &f.ctl {
                Ctl::Text(t) => {
                    t.on_text_updated(move |_| c());
                }
                Ctl::Check(b) => {
                    b.on_toggled(move |_| c());
                }
            }
        }
        const EXPORT: i32 = 5001;
        const IMPORT: i32 = 5002;
        export.on_click(move |_| dialog.end_modal(EXPORT));
        import.on_click(move |_| dialog.end_modal(IMPORT));
        let answer = dialog.show_modal();
        let (value, _) = read(&base, &fields);
        let rerun = rerun.get_value();
        dialog.destroy();
        match answer {
            ID_OK => {
                let mut c = value;
                c["version"] = base["version"].clone();
                match self.run("precast_config.apply", json!({ "config": c })) {
                    Ok(v) => {
                        frame.set_status_text(&format!("Precast configuration version {v}"), 0);
                        if rerun && !older.is_empty() {
                            match self.run("precast.resplit", json!({ "assembly_ids": older })) {
                                Ok(a) => {
                                    self.refresh_scene(&frame);
                                    frame.set_status_text(&format!("Re-split: {} assemblies", a.as_array().map_or(0, Vec::len)), 0);
                                }
                                Err(e) => error(&frame, &format!("The assemblies were not re-split:\n{e}")),
                            }
                        }
                    }
                    Err(e) => error(&frame, &format!("The configuration was not changed:\n{e}")),
                }
            }
            EXPORT | IMPORT => {
                let save = answer == EXPORT;
                let d = FileDialog::builder(&frame)
                    .with_message(if save { "Export Precast Configuration" } else { "Import Precast Configuration" })
                    .with_wildcard("Precast configuration (*.json)|*.json")
                    .with_style(if save { FileDialogStyle::Save | FileDialogStyle::OverwritePrompt } else { FileDialogStyle::Open | FileDialogStyle::FileMustExist })
                    .build();
                if d.show_modal() == ID_OK
                    && let Some(path) = d.get_path()
                {
                    let r = self.run(if save { "precast_config.export" } else { "precast_config.import" }, json!({ "file": path }));
                    match (r, save) {
                        (Ok(_), true) => frame.set_status_text(&format!("Exported to {path}"), 0),
                        (Ok(i), false) => {
                            let missing: Vec<String> = i["missing"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_owned)).collect();
                            if !missing.is_empty() {
                                error(&frame, &format!("Imported. Load these components, which the configuration names:\n{}", missing.join("\n")));
                            }
                            // Open again on the imported values.
                            self.precast_configuration()
                        }
                        (Err(e), _) => error(&frame, &e),
                    }
                }
            }
            _ => {}
        }
    }

    /// Concrete › Precast Detailing › Split (STR-PRE-002): the selected walls and floors.
    pub(crate) fn precast_split(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let picked: Vec<(ElementId, bool)> = self
            .selection()
            .into_iter()
            .filter_map(|id| {
                let e = store.get(id)?;
                if reviera_core::walls::wall_recipe(e).is_some() {
                    Some((id, true))
                } else if reviera_core::floors::slab_recipe(e).is_some_and(|(k, _)| k == reviera_core::floors::SlabKind::Floor) {
                    Some((id, false))
                } else {
                    None
                }
            })
            .collect();
        if picked.is_empty() {
            return frame.set_status_text("Split: select walls or floors first", 0);
        }
        let floors = picked.iter().any(|p| !p.1);
        let walls = picked.iter().any(|p| p.1);
        if floors && walls {
            return frame.set_status_text("Split: select walls or floors, not both", 0);
        }
        let kinds: Vec<(&str, &str)> = if floors {
            vec![("solid", "Solid slab"), ("hollow_core", "Hollow-core slab"), ("lattice_girder", "Lattice-girder slab")]
        } else {
            vec![("solid", "Solid wall"), ("double", "Double wall")]
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Split");
        let kind = f.choice(if floors { "Slab kind" } else { "Wall kind" }, &kinds.iter().map(|k| k.1.to_owned()).collect::<Vec<_>>(), 0);
        if !f.show("Pieces follow the precast configuration's segmentation rules (Precast Settings › Configuration)", "Split") {
            f.dialog.destroy();
            return;
        }
        let k = kinds[kind.get_selection().map_or(0, |i| i as usize).min(kinds.len() - 1)].0;
        f.dialog.destroy();
        let ids: Vec<ElementId> = picked.iter().map(|p| p.0).collect();
        match self.run("precast.split", json!({ "element_ids": ids, "slab_kind": k })) {
            Ok(a) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Split into {} pieces", a.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) if e.contains("precast.already_split") => {
                // Offer to re-split the assemblies instead.
                let assemblies: Vec<u64> = store
                    .iter()
                    .filter(|a| reviera_core::precast::assembly_recipe(a).is_some_and(|r| picked.iter().any(|(p, _)| store.get(*p).is_some_and(|s| s.guid().to_string() == r.source))))
                    .map(|a| a.id().0)
                    .collect();
                let ask = MessageDialog::builder(&frame, "These elements are already split. Re-split their assemblies with the current rules?", "Split")
                    .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                    .build();
                if ask.show_modal() == ID_YES {
                    self.precast_resplit_ids(assemblies);
                }
            }
            Err(e) => error(&frame, &format!("Not split:\n{e}")),
        }
    }

    /// The precast assemblies the selection names: assemblies, their parts or the split elements.
    fn selected_assemblies(&mut self) -> Vec<u64> {
        let Some(store) = self.store_now() else { return Vec::new() };
        let sel = self.selection();
        store
            .iter()
            .filter(|a| {
                reviera_core::precast::assembly_recipe(a).is_some_and(|r| {
                    sel.contains(&a.id()) || sel.iter().any(|s| store.get(*s).is_some_and(|e| e.guid().to_string() == r.source || r.parts.contains(&e.guid().to_string())))
                })
            })
            .map(|a| a.id().0)
            .collect()
    }

    /// Concrete › Precast Detailing › Auto Reinforce (STR-PRE-003).
    pub(crate) fn precast_reinforce(&mut self) {
        let frame = self.frame;
        let assemblies = self.selected_assemblies();
        if assemblies.is_empty() {
            return frame.set_status_text("Auto Reinforce: select precast assemblies, their pieces or the split elements", 0);
        }
        let offered = match self.run("precast.reinforcement_types", json!({ "assembly_ids": assemblies })) {
            Ok(o) => o,
            Err(e) => return error(&frame, &format!("Auto Reinforce:\n{e}")),
        };
        let list = |k: &str| -> Vec<(String, String)> {
            offered[k].as_array().into_iter().flatten().map(|t| (t["id"].as_str().unwrap_or("").to_owned(), format!("{} — {}", t["id"].as_str().unwrap_or(""), t["name"].as_str().unwrap_or("")))).collect()
        };
        let (area, edge) = (list("area_types"), list("edge_types"));
        let t = &offered["thickness"];
        let range = format!("{:.0}–{:.0} mm", t[0].as_f64().unwrap_or(0.0) * 1000.0, t[1].as_f64().unwrap_or(0.0) * 1000.0);
        if area.is_empty() {
            return error(&frame, &format!("No area reinforcement type holds these pieces ({range}). Add one in Precast Settings › Configuration › Reinforcement."));
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Auto Reinforce");
        let a = f.choice("Area reinforcement", &area.iter().map(|x| x.1.clone()).collect::<Vec<_>>(), 0);
        let mut edge_names = vec!["None".to_owned()];
        edge_names.extend(edge.iter().map(|x| x.1.clone()));
        let e = (!edge.is_empty()).then(|| f.choice("Edge U-bars", &edge_names, 1));
        if !f.show(&format!("{} piece(s), {range}: only the types whose thickness range holds them", assemblies.len()), "OK") {
            f.dialog.destroy();
            return;
        }
        let mut args = json!({ "assembly_ids": assemblies, "area_type_id": area[a.get_selection().map_or(0, |i| i as usize).min(area.len() - 1)].0 });
        if let Some(k) = e.as_ref().and_then(|c| c.get_selection()).and_then(|i| (i as usize).checked_sub(1)) {
            args["edge_type_id"] = json!(edge[k.min(edge.len() - 1)].0);
        }
        f.dialog.destroy();
        match self.run("precast.reinforce", args) {
            Ok(r) => {
                self.refresh_scene(&frame);
                let lines: Vec<String> = r["assemblies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| {
                        let n = |k: &str| a[k].as_array().map_or(0, Vec::len);
                        let mut l = format!("{}: {} set(s) made, {} replaced, {} kept", a["mark"].as_str().unwrap_or(""), n("created"), a["removed"].as_u64().unwrap_or(0), n("kept"));
                        for k in a["kept"].as_array().into_iter().flatten() {
                            l += &format!("\n    set {} {}", k["id"], k["why"].as_str().unwrap_or(""));
                        }
                        for s in a["skipped"].as_array().into_iter().flatten() {
                            l += &format!("\n    skipped: {}", s.as_str().unwrap_or(""));
                        }
                        l
                    })
                    .collect();
                MessageDialog::builder(&frame, &lines.join("\n"), "Auto Reinforce").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &format!("Not reinforced:\n{e}")),
        }
    }

    /// Concrete › Precast Detailing › Custom Fabric Sheets (STR-PRE-004).
    pub(crate) fn precast_custom_fabric(&mut self) {
        let frame = self.frame;
        let assemblies = self.selected_assemblies();
        if assemblies.is_empty() {
            return frame.set_status_text("Custom Fabric Sheets: select precast assemblies, their pieces or the split elements", 0);
        }
        let rules = self.run("precast_config.get", json!({ "path": "fabric_rules" })).unwrap_or_default();
        let list: Vec<(String, String)> = rules.as_array().into_iter().flatten().map(|r| (r["id"].as_str().unwrap_or("").to_owned(), format!("{} — {}", r["id"].as_str().unwrap_or(""), r["name"].as_str().unwrap_or("")))).collect();
        if list.is_empty() {
            return error(&frame, "The configuration has no custom fabric sheet rule. Add one in Precast Settings › Configuration.");
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Custom Fabric Sheets");
        let rule = f.choice("Rule", &list.iter().map(|x| x.1.clone()).collect::<Vec<_>>(), 0);
        if !f.show(&format!("{} piece(s): sheets shaped to each, cut round openings, lapped where a piece is larger than a sheet", assemblies.len()), "OK") {
            f.dialog.destroy();
            return;
        }
        let id = list[rule.get_selection().map_or(0, |i| i as usize).min(list.len() - 1)].0.clone();
        f.dialog.destroy();
        match self.run("precast.generate_custom_fabric", json!({ "assembly_ids": assemblies, "rule_id": id })) {
            Ok(r) => {
                self.refresh_scene(&frame);
                let lines: Vec<String> = r
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| {
                        let mut l = format!(
                            "{}: {} sheet(s), {} replaced, {} kept, {} short wire(s) left out",
                            a["mark"].as_str().unwrap_or(""),
                            a["sheets"].as_array().map_or(0, Vec::len),
                            a["removed"].as_u64().unwrap_or(0),
                            a["kept"].as_array().map_or(0, Vec::len),
                            a["dropped_wires"].as_u64().unwrap_or(0)
                        );
                        for s in a["skipped"].as_array().into_iter().flatten() {
                            l += &format!("\n    skipped: {}", s.as_str().unwrap_or(""));
                        }
                        l
                    })
                    .collect();
                MessageDialog::builder(&frame, &lines.join("\n"), "Custom Fabric Sheets").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &format!("No sheets made:\n{e}")),
        }
    }

    /// Re-split: the selected precast assemblies (or those of the selected elements).
    pub(crate) fn precast_resplit(&mut self) {
        let assemblies = self.selected_assemblies();
        if assemblies.is_empty() {
            return self.frame.set_status_text("Re-split: select precast assemblies, their pieces or the split elements", 0);
        }
        self.precast_resplit_ids(assemblies);
    }

    fn precast_resplit_ids(&mut self, assemblies: Vec<u64>) {
        let frame = self.frame;
        match self.run("precast.resplit", json!({ "assembly_ids": assemblies })) {
            Ok(a) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Re-split into {} pieces", a.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("Not re-split:\n{e}")),
        }
    }
}
