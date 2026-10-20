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
    match (v, unit) {
        (Some(Value::Number(n)), Unit::Mm) => format!("{}", (n.as_f64().unwrap_or(0.0) * 1e6).round() / 1000.0),
        (Some(Value::Number(n)), _) => format!("{}", n.as_f64().unwrap_or(0.0)),
        (Some(Value::String(s)), _) => s.clone(),
        _ => String::new(),
    }
}

/// The dialog's values over `base`; the paths whose text is not a number.
fn read(base: &Value, fields: &[Field]) -> (Value, Vec<String>) {
    let mut v = base.clone();
    let mut bad = Vec::new();
    for f in fields {
        let value = match (&f.ctl, f.unit) {
            (Ctl::Check(c), _) => json!(c.get_value()),
            (Ctl::Text(t), Unit::Text) => json!(t.get_value()),
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
            for node in ["Reinforcement", "Connections", "Shop Drawing"] {
                page(node, "", &[], true, &mut fields, &mut pages);
            }
        }
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
                    let is_bad = wrong.iter().any(|(p, _)| *p == f.path);
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

    /// Re-split: the selected precast assemblies (or those of the selected elements).
    pub(crate) fn precast_resplit(&mut self) {
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let assemblies: Vec<u64> = store
            .iter()
            .filter(|a| {
                reviera_core::precast::assembly_recipe(a).is_some_and(|r| {
                    sel.contains(&a.id()) || sel.iter().any(|s| store.get(*s).is_some_and(|e| e.guid().to_string() == r.source || r.parts.contains(&e.guid().to_string())))
                })
            })
            .map(|a| a.id().0)
            .collect();
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
