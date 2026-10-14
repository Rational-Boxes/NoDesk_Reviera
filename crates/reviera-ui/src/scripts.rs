// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Scripts in the window (TECH_STACK §3.5): the Scripts menu (Run Script…, Import into
//! Library…, Manage Library…, then the known scripts), and the dialog a script's form
//! (`reviera.ui.form`) shows as.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use reviera_core::units::UnitSettings;
use reviera_lua::{FieldKind, Form};
use serde_json::{Map, Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// The known scripts' menu items, one id each.
pub(crate) const ID_PLUGIN_FIRST: i32 = wxdragon::id::ID_HIGHEST + 160;
pub(crate) const MAX_PLUGINS: usize = 40;

/// One input of a form dialog.
enum Input {
    Text(TextCtrl),
    Check(CheckBox),
    Choice(Choice, Vec<String>),
}

/// Shows a script's form as a modal dialog: a field per row, the script's message on top and
/// the last problem under it. The raw answers (lengths and numbers as typed), or `None` when
/// cancelled.
pub(crate) fn form_dialog(
    frame: &Frame,
    form: &Form,
    current: &Map<String, Value>,
    problem: Option<&str>,
    units: &UnitSettings,
) -> Option<Map<String, Value>> {
    let title = form.title.as_deref().unwrap_or("Script");
    let dialog = Dialog::builder(frame, title)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    if let Some(m) = &form.message {
        let text = StaticText::builder(&dialog).with_label(m).build();
        text.wrap(420);
        sizer.add(&text, 0, SizerFlag::All | SizerFlag::Expand, 10);
    }
    if let Some(p) = problem {
        let text = StaticText::builder(&dialog)
            .with_label(&format!("⚠ {p}"))
            .build();
        text.set_foreground_color(Colour::rgb(200, 40, 40));
        text.wrap(420);
        sizer.add(
            &text,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
            10,
        );
    }
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(6)
        .with_hgap(10)
        .build();
    grid.add_growable_col(1, 1);
    let mut inputs: Vec<(String, Input)> = Vec::new();
    for f in &form.fields {
        let value = current.get(&f.key).cloned().unwrap_or(Value::Null);
        let label = StaticText::builder(&dialog).with_label(f.label()).build();
        let input = match f.kind {
            FieldKind::Bool => {
                let c = CheckBox::builder(&dialog)
                    .with_value(value.as_bool().unwrap_or(false))
                    .build();
                if let Some(h) = &f.help {
                    c.set_tooltip(h);
                }
                grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&c, 0, SizerFlag::AlignCenterVertical, 0);
                Input::Check(c)
            }
            FieldKind::Choice => {
                let c = Choice::builder(&dialog)
                    .with_choices(f.choices.clone())
                    .build();
                let at = value
                    .as_str()
                    .and_then(|v| f.choices.iter().position(|c| c == v))
                    .unwrap_or(0);
                c.set_selection(at as u32);
                if let Some(h) = &f.help {
                    c.set_tooltip(h);
                }
                grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&c, 1, SizerFlag::Expand, 0);
                Input::Choice(c, f.choices.clone())
            }
            _ => {
                let shown = match (&value, &f.kind) {
                    (Value::Number(n), FieldKind::Length) => reviera_core::units::format(
                        n.as_f64().unwrap_or(0.0),
                        "length",
                        units,
                        None,
                    )
                    .unwrap_or_else(|_| n.to_string()),
                    (Value::String(s), _) => s.clone(),
                    (Value::Null, _) => String::new(),
                    (v, _) => v.to_string(),
                };
                let t = TextCtrl::builder(&dialog).with_value(&shown).build();
                t.set_min_size(Size::new(220, -1));
                let mut tip = f.help.clone().unwrap_or_default();
                let range = match (f.min, f.max) {
                    (Some(a), Some(b)) => format!("{a} to {b}"),
                    (Some(a), None) => format!("at least {a}"),
                    (None, Some(b)) => format!("at most {b}"),
                    (None, None) => String::new(),
                };
                if !range.is_empty() {
                    if !tip.is_empty() {
                        tip.push_str(" — ");
                    }
                    tip.push_str(&range);
                }
                if !tip.is_empty() {
                    t.set_tooltip(&tip);
                }
                grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&t, 1, SizerFlag::Expand, 0);
                Input::Text(t)
            }
        };
        inputs.push((f.key.clone(), input));
    }
    sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
    let ok = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label("OK")
        .build();
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("Cancel")
        .build();
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    buttons.add(&cancel, 0, SizerFlag::All, 6);
    buttons.add(&ok, 0, SizerFlag::All, 6);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_affirmative_id(ID_OK);
    ok.set_default();
    let answers = (dialog.show_modal() == ID_OK).then(|| {
        inputs
            .iter()
            .map(|(key, input)| {
                let v = match input {
                    Input::Text(t) => Value::from(t.get_value()),
                    Input::Check(c) => Value::Bool(c.get_value()),
                    Input::Choice(c, choices) => Value::from(
                        c.get_selection()
                            .and_then(|i| choices.get(i as usize))
                            .cloned()
                            .unwrap_or_default(),
                    ),
                };
                (key.clone(), v)
            })
            .collect()
    });
    dialog.destroy();
    answers
}

/// Installs the form dialog as the application's form handler; `units` holds the units of the
/// model a script runs on, set before each run.
pub(crate) fn install_form_handler(gui: &mut Gui, units: Rc<RefCell<UnitSettings>>) {
    let frame = gui.frame;
    gui.app
        .set_form_handler(Box::new(move |form, current, problem| {
            form_dialog(&frame, form, current, problem, &units.borrow())
        }));
}

impl Gui {
    /// Runs a command of the script family with the model's units for its forms; reports what
    /// the script printed, or why it failed, and refreshes the model.
    fn run_script_command(&mut self, command: &str, args: Value, what: &str) {
        let frame = self.frame;
        if let Some(d) = self.doc.and_then(|d| self.app.document(d)) {
            *self.script_units.borrow_mut() = reviera_core::units::settings_of(d.doc.store());
        }
        match self.run(command, args) {
            Ok(v) => {
                let output: Vec<String> = v["output"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l.as_str().map(str::to_owned))
                    .collect();
                if output.is_empty() {
                    frame.set_status_text(&format!("{what} ran"), 0);
                } else {
                    frame.set_status_text(output.last().expect("one"), 0);
                    if output.len() > 1 {
                        MessageDialog::builder(&frame, &output.join("\n"), what)
                            .with_style(
                                MessageDialogStyle::OK | MessageDialogStyle::IconInformation,
                            )
                            .build()
                            .show_modal();
                    }
                }
            }
            Err(e) => error(&frame, &format!("{what} did not run:\n{e}")),
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        self.selection_changed();
    }

    /// Runs a known script by name (Scripts menu, Manage Library).
    pub(crate) fn run_plugin(&mut self, name: &str) {
        if self.doc.is_none() {
            return error(&self.frame, "Open or create a project first.");
        }
        let title = self
            .app
            .plugins()
            .into_iter()
            .find(|p| p.name == name)
            .map_or_else(|| name.to_owned(), |p| p.title);
        self.run_script_command("plugin.run", json!({ "name": name }), &title);
    }

    /// The plugin of a Scripts menu item.
    pub(crate) fn run_plugin_item(&mut self, id: i32) {
        let plugins = self.app.plugins();
        if let Some(p) = usize::try_from(id - ID_PLUGIN_FIRST)
            .ok()
            .and_then(|i| plugins.get(i))
        {
            let name = p.name.clone();
            self.run_plugin(&name);
        }
    }

    /// Scripts › Run Script…: a `.lua` file, run once.
    pub(crate) fn open_script(&mut self) {
        let frame = self.frame;
        if self.doc.is_none() {
            return error(&frame, "Open or create a project first.");
        }
        let dialog = FileDialog::builder(&frame)
            .with_message("Run Script")
            .with_wildcard("Lua scripts (*.lua)|*.lua|All files (*.*)|*.*")
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(path) = dialog.get_path() else {
            return;
        };
        let name = Path::new(&path)
            .file_stem()
            .map_or_else(|| "Script".to_owned(), |s| s.to_string_lossy().into_owned());
        self.run_script_command("script.run", json!({ "file": path }), &name);
    }

    /// Scripts › Import into Library…: a script file or a plugin folder.
    pub(crate) fn import_script(&mut self) {
        let frame = self.frame;
        let kinds = [
            "A script file (.lua)",
            "A plugin folder (holding plugin.lua)",
        ];
        let choose = SingleChoiceDialog::builder(
            &frame,
            "What to import into the script library:",
            "Import Script",
            &kinds,
        )
        .build();
        if choose.show_modal() != ID_OK {
            return;
        }
        let path = if choose.get_selection() == 0 {
            let d = FileDialog::builder(&frame)
                .with_message("Import Script")
                .with_wildcard("Lua scripts (*.lua)|*.lua")
                .build();
            if d.show_modal() != ID_OK {
                return;
            }
            d.get_path()
        } else {
            let d = DirDialog::builder(&frame, "Import Plugin Folder", "").build();
            if d.show_modal() != ID_OK {
                return;
            }
            d.get_path()
        };
        let Some(path) = path else { return };
        let mut result = self.run("script.import", json!({ "path": path }));
        if let Err(e) = &result
            && e.contains("script.exists")
            && super::ask(
                &frame,
                "A script of that name is in the library. Replace it?",
            )
        {
            result = self.run("script.import", json!({ "path": path, "overwrite": true }));
        }
        match result {
            Ok(v) => frame.set_status_text(
                &format!(
                    "Imported {} into the script library",
                    v["title"].as_str().unwrap_or("the script")
                ),
                0,
            ),
            Err(e) => error(&frame, &format!("The script was not imported:\n{e}")),
        }
        self.refresh_scripts_menu();
    }

    /// Scripts › Manage Library…: the known scripts, to run, remove or add to.
    pub(crate) fn manage_scripts(&mut self) {
        const RUN: i32 = wxdragon::id::ID_HIGHEST + 1;
        const REMOVE: i32 = wxdragon::id::ID_HIGHEST + 2;
        const IMPORT: i32 = wxdragon::id::ID_HIGHEST + 3;
        let frame = self.frame;
        let mut keep = None;
        loop {
            let plugins = self.app.plugins();
            let library = reviera_lua::library_dir();
            let dialog = Dialog::builder(&frame, "Script Library")
                .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
                .with_size(560, 400)
                .build();
            let list = ListBox::builder(&dialog).build();
            for p in &plugins {
                let place = match &p.path {
                    None => "built-in".to_owned(),
                    Some(path) if library.as_ref().is_some_and(|l| path.starts_with(l)) => {
                        "library".to_owned()
                    }
                    Some(path) => path.display().to_string(),
                };
                list.append(&format!("{} — {} ({place})", p.title, p.name));
            }
            let about = StaticText::builder(&dialog).with_label(" ").build();
            let select = |i: usize| {
                if let Some(p) = plugins.get(i) {
                    about.set_label(&p.description);
                    about.wrap(520);
                }
            };
            if !plugins.is_empty() {
                let at = keep.unwrap_or(0).min(plugins.len() - 1);
                list.set_selection(at as u32, true);
                select(at);
            }
            {
                let (list, about, descriptions) = (
                    list,
                    about,
                    plugins
                        .iter()
                        .map(|p| p.description.clone())
                        .collect::<Vec<_>>(),
                );
                list.on_selection_changed(move |_| {
                    if let Some(d) = list
                        .get_selection()
                        .and_then(|i| descriptions.get(i as usize))
                    {
                        about.set_label(d);
                        about.wrap(520);
                    }
                });
            }
            let buttons = BoxSizer::builder(Orientation::Horizontal).build();
            for (id, label) in [
                (IMPORT, "Import…"),
                (REMOVE, "Remove"),
                (RUN, "Run"),
                (ID_CANCEL, "Close"),
            ] {
                let b = Button::builder(&dialog)
                    .with_id(id)
                    .with_label(label)
                    .build();
                if id != ID_CANCEL {
                    b.on_click(move |_| dialog.end_modal(id));
                }
                if id == IMPORT {
                    buttons.add(&b, 0, SizerFlag::All, 6);
                    buttons.add_stretch_spacer(1);
                } else {
                    buttons.add(&b, 0, SizerFlag::All, 6);
                }
            }
            let sizer = BoxSizer::builder(Orientation::Vertical).build();
            sizer.add(&list, 1, SizerFlag::All | SizerFlag::Expand, 8);
            sizer.add(
                &about,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
                10,
            );
            sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
            dialog.set_sizer(sizer, true);
            let answer = dialog.show_modal();
            let chosen = list.get_selection().map(|i| i as usize);
            dialog.destroy();
            keep = chosen;
            let plugin = chosen.and_then(|i| plugins.get(i));
            match answer {
                RUN => {
                    if let Some(p) = plugin {
                        let name = p.name.clone();
                        self.run_plugin(&name);
                    }
                    return;
                }
                REMOVE => {
                    let Some(p) = plugin else { continue };
                    if p.path.is_none() {
                        error(&frame, "Built-in scripts cannot be removed.");
                        continue;
                    }
                    if super::ask(&frame, &format!("Remove {} from the library?", p.title))
                        && let Err(e) = self.run("script.remove", json!({ "name": p.name }))
                    {
                        error(&frame, &format!("The script was not removed:\n{e}"));
                    }
                    self.refresh_scripts_menu();
                }
                IMPORT => self.import_script(),
                _ => return,
            }
        }
    }

    /// Lists the known scripts at the end of the Scripts menu.
    pub(crate) fn refresh_scripts_menu(&mut self) {
        let Some(anchor) = self.ribbon.id_of("ui.scripts.manage") else {
            return;
        };
        let Some(bar) = self.frame.get_menu_bar() else {
            return;
        };
        let Some((_, menu)) = bar.find_item_and_menu(anchor) else {
            return;
        };
        for k in 0..MAX_PLUGINS {
            let id = ID_PLUGIN_FIRST + k as i32;
            if menu.find_item(id).is_some() {
                menu.delete(id);
            }
        }
        for (k, p) in self.app.plugins().iter().take(MAX_PLUGINS).enumerate() {
            menu.append(
                ID_PLUGIN_FIRST + k as i32,
                &p.title,
                &p.description,
                ItemKind::Normal,
            );
        }
    }
}
