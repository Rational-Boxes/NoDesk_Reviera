// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Loads in the window (STR-LC-001, STR-LOAD-001, STR-BC-001): Analyze › Loads › Load Cases opens
//! one dialog with the design code, load natures, cases, combinations and usages; changes apply
//! as they are made, and Cancel undoes them all. Point Load, Line Load, Area Load and Point
//! Support ask for their values and then place on what is clicked in a plan at its level: the
//! nearest analytical node or member, or the panel under the click.

use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

/// A message box over a dialog.
fn warn(parent: &dyn WxWidget, message: &str) {
    let d = MessageDialog::builder(parent, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}

/// A form field.
enum Field {
    Text(&'static str, String),
    Choice(&'static str, Vec<String>, usize),
    Check(&'static str, bool),
}

/// A field's answer.
enum Answer {
    Text(String),
    Choice(usize),
    Check(bool),
}

impl Answer {
    fn text(&self) -> String {
        match self {
            Answer::Text(s) => s.trim().to_owned(),
            _ => String::new(),
        }
    }
    fn choice(&self) -> usize {
        match self {
            Answer::Choice(i) => *i,
            _ => 0,
        }
    }
    fn check(&self) -> bool {
        matches!(self, Answer::Check(true))
    }
    fn number(&self) -> f64 {
        self.text().parse().unwrap_or(0.0)
    }
}

/// A small modal form: a label and a control per field; `None` when cancelled.
fn form(parent: &dyn WxWidget, title: &str, fields: Vec<Field>) -> Option<Vec<Answer>> {
    let dialog = Dialog::builder(parent, title)
        .with_style(DialogStyle::DefaultDialogStyle)
        .build();
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(6)
        .with_hgap(10)
        .build();
    grid.add_growable_col(1, 1);
    enum Ctl {
        Text(TextCtrl),
        Choice(Choice),
        Check(CheckBox),
    }
    let mut ctls = Vec::new();
    for f in fields {
        let (label, ctl) = match f {
            Field::Text(l, v) => {
                let t = TextCtrl::builder(&dialog).with_value(&v).build();
                t.set_min_size(Size::new(260, -1));
                (l, Ctl::Text(t))
            }
            Field::Choice(l, items, at) => {
                let c = Choice::builder(&dialog).with_choices(items).build();
                c.set_selection(at as u32);
                (l, Ctl::Choice(c))
            }
            Field::Check(l, v) => {
                let c = CheckBox::builder(&dialog).with_label("").build();
                c.set_value(v);
                (l, Ctl::Check(c))
            }
        };
        let t = StaticText::builder(&dialog).with_label(label).build();
        grid.add(&t, 0, SizerFlag::AlignCenterVertical, 0);
        match &ctl {
            Ctl::Text(t) => {
                grid.add(t, 1, SizerFlag::Expand, 0);
            }
            Ctl::Choice(c) => {
                grid.add(c, 1, SizerFlag::Expand, 0);
            }
            Ctl::Check(c) => {
                grid.add(c, 0, SizerFlag::empty(), 0);
            }
        }
        ctls.push(ctl);
    }
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 12);
    let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
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
    let answered = dialog.show_modal() == ID_OK;
    let out = answered.then(|| {
        ctls.iter()
            .map(|c| match c {
                Ctl::Text(t) => Answer::Text(t.get_value()),
                Ctl::Choice(c) => Answer::Choice(c.get_selection().unwrap_or(0) as usize),
                Ctl::Check(c) => Answer::Check(c.get_value()),
            })
            .collect()
    });
    dialog.destroy();
    out
}

const LIMIT_STATES: [&str; 7] = [
    "ultimate",
    "serviceability",
    "quasi_permanent",
    "frequent",
    "rare",
    "accidental",
    "special",
];

/// What a placement tool places.
#[derive(Debug, Clone)]
pub enum LoadToolKind {
    Point { case: Value, forces: [f64; 6] },
    Line { case: Value, start: [f64; 3], end: [f64; 3] },
    Area { case: Value, value: [f64; 3] },
    Support { state: String },
}

/// A load or support placement tool: its level and what it places.
#[derive(Debug, Clone)]
pub struct LoadTool {
    level: ElementId,
    kind: LoadToolKind,
    placed: usize,
}

/// A list control in a dialog page with its columns.
fn list(page: &Panel, columns: &[(&str, i32)]) -> ListCtrl {
    let l = ListCtrl::builder(page)
        .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
        .build();
    for (k, (h, w)) in columns.iter().enumerate() {
        l.insert_column(k as i64, h, ListColumnFormat::Left, *w);
    }
    l
}

fn fill(l: &ListCtrl, rows: &[Vec<String>]) {
    l.delete_all_items();
    for (k, r) in rows.iter().enumerate() {
        l.insert_item(k as i64, r.first().map(String::as_str).unwrap_or(""), None);
        for (c, t) in r.iter().enumerate().skip(1) {
            l.set_item_text_by_column(k as i64, c as i32, t);
        }
    }
}

fn selected(l: &ListCtrl) -> Option<usize> {
    let i = l.get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
    (i >= 0).then_some(i as usize)
}

impl Gui {
    /// Analyze › Loads › Load Cases (`load_case.manage`): the dialog, on `tab` (code, natures,
    /// cases, combinations or usages).
    pub(crate) fn load_cases_dialog(gui: &std::rc::Rc<std::cell::RefCell<Gui>>, tab: &str) {
        // Nothing of the window's state is borrowed while the dialog is open: each change
        // borrows it for its command.
        let frame = gui.borrow().frame;
        if gui.borrow_mut().run("load_case.list", json!({})).is_err() {
            return frame.set_status_text("Open a model first", 0);
        }
        let dialog = Dialog::builder(&frame, "Load Cases and Combinations")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let book = Notebook::builder(&dialog).build();
        let page = |name: &str| {
            let p = Panel::builder(&book).build();
            book.add_page(&p, name, false, None);
            p
        };
        // Design code.
        let code_page = page("Design Code");
        let template = StaticText::builder(&code_page).with_label("").build();
        let code_asce = Button::builder(&code_page)
            .with_label("Use ASCE 7 (US)")
            .build();
        let code_en = Button::builder(&code_page)
            .with_label("Use EN 1990 (Eurocode)")
            .build();
        // Natures.
        let nature_page = page("Load Natures");
        let natures = list(
            &nature_page,
            &[("Name", 160), ("Symbol", 70), ("Action type", 110), ("IFC source", 170), ("Source", 70)],
        );
        let add_nature = Button::builder(&nature_page)
            .with_label("Add Nature…")
            .build();
        // Cases.
        let case_page = page("Load Cases");
        let cases = list(
            &case_page,
            &[("Name", 140), ("Nature", 140), ("Category", 110), ("Self-weight", 90), ("Loads", 60)],
        );
        let add_case = Button::builder(&case_page).with_label("Add…").build();
        let edit_case = Button::builder(&case_page).with_label("Edit…").build();
        let delete_case = Button::builder(&case_page).with_label("Delete").build();
        // Combinations.
        let combo_page = page("Load Combinations");
        let combos = list(
            &combo_page,
            &[("Name", 140), ("Formula", 300), ("Limit state", 110), ("Usages", 140)],
        );
        let add_combo = Button::builder(&combo_page).with_label("Add…").build();
        let edit_combo = Button::builder(&combo_page).with_label("Edit…").build();
        let delete_combo = Button::builder(&combo_page).with_label("Delete").build();
        let generate = Button::builder(&combo_page)
            .with_label("Generate…")
            .build();
        // Usages.
        let usage_page = page("Load Usages");
        let usages = list(&usage_page, &[("Usage", 160), ("Combinations", 300)]);
        let rename_usage = Button::builder(&usage_page).with_label("Rename…").build();
        let delete_usage = Button::builder(&usage_page).with_label("Delete").build();

        let lay = |p: &Panel, l: Option<&ListCtrl>, top: Option<&StaticText>, buttons: &[&Button]| {
            let s = BoxSizer::builder(Orientation::Vertical).build();
            if let Some(t) = top {
                s.add(t, 0, SizerFlag::All | SizerFlag::Expand, 8);
            }
            if let Some(l) = l {
                s.add(l, 1, SizerFlag::All | SizerFlag::Expand, 6);
            }
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            for b in buttons {
                row.add(*b, 0, SizerFlag::Right, 4);
            }
            s.add_sizer(&row, 0, SizerFlag::All, 6);
            p.set_sizer(s, true);
        };
        lay(&code_page, None, Some(&template), &[&code_asce, &code_en]);
        lay(&nature_page, Some(&natures), None, &[&add_nature]);
        lay(&case_page, Some(&cases), None, &[&add_case, &edit_case, &delete_case]);
        lay(
            &combo_page,
            Some(&combos),
            None,
            &[&add_combo, &edit_combo, &delete_combo, &generate],
        );
        lay(&usage_page, Some(&usages), None, &[&rename_usage, &delete_usage]);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&book, 1, SizerFlag::All | SizerFlag::Expand, 8);
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("Cancel")
            .build();
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let note = StaticText::builder(&dialog)
            .with_label("Changes apply as you make them; Cancel undoes them all.")
            .build();
        buttons.add(&note, 1, SizerFlag::AlignCenterVertical | SizerFlag::Left, 10);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer(sizer, true);
        dialog.set_size(Size::new(860, 520));
        dialog.set_affirmative_id(ID_OK);
        let at = ["code", "natures", "cases", "combinations", "usages"]
            .iter()
            .position(|t| *t == tab)
            .unwrap_or(2);
        book.set_selection(at);

        // Every change made, to undo on Cancel; the report behind the lists.
        let state = std::rc::Rc::new(std::cell::RefCell::new((0usize, Value::Null)));
        let gui_weak = std::rc::Rc::downgrade(gui);
        let gui_rc = gui.clone();
        let gui = gui_weak;
        let refresh = {
            let (state, gui) = (state.clone(), gui.clone());
            let (template, natures, cases, combos, usages) = (template, natures, cases, combos, usages);
            move || {
                let Some(g) = gui.upgrade() else { return };
                let Ok(r) = g.borrow_mut().run("load_case.list", json!({})) else {
                    return;
                };
                template.set_label(&format!(
                    "Code template: {}. Natures come from the template; combinations are generated from its rules, offline.",
                    match r["template"].as_str() {
                        Some("asce7") => "ASCE/SEI 7-22 (strength design)",
                        Some("en1990") => "EN 1990 (expression 6.10, recommended factors)",
                        Some(t) => t,
                        None => "the region's",
                    }
                ));
                let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
                fill(
                    &natures,
                    &r["natures"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|n| vec![s(&n["name"]), s(&n["symbol"]), s(&n["action_type"]), s(&n["action_source"]), s(&n["source"])])
                        .collect::<Vec<_>>(),
                );
                fill(
                    &cases,
                    &r["cases"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|c| {
                            vec![
                                s(&c["name"]),
                                s(&c["nature"]),
                                s(&c["category"]),
                                if c["self_weight"] == true { "Yes".into() } else { String::new() },
                                c["loads"].to_string(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                );
                let cl = r["combinations"].as_array().cloned().unwrap_or_default();
                fill(
                    &combos,
                    &cl.iter()
                        .map(|c| {
                            let u: Vec<String> = c["usages"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(&s)
                                .collect();
                            vec![s(&c["name"]), s(&c["formula"]), s(&c["limit_state"]), u.join(", ")]
                        })
                        .collect::<Vec<_>>(),
                );
                let mut tags: Vec<(String, Vec<String>)> = Vec::new();
                for c in &cl {
                    for u in c["usages"].as_array().into_iter().flatten() {
                        let u = s(u);
                        match tags.iter_mut().find(|(t, _)| *t == u) {
                            Some((_, names)) => names.push(s(&c["name"])),
                            None => tags.push((u, vec![s(&c["name"])])),
                        }
                    }
                }
                fill(
                    &usages,
                    &tags.iter().map(|(t, n)| vec![t.clone(), n.join(", ")]).collect::<Vec<_>>(),
                );
                state.borrow_mut().1 = r;
            }
        };
        refresh();
        let refresh = std::rc::Rc::new(refresh);
        // Runs a command, counting it for Cancel, and refreshes the lists.
        let apply = {
            let (state, gui, refresh, dialog) = (state.clone(), gui.clone(), refresh.clone(), dialog);
            std::rc::Rc::new(move |name: &str, args: Value| -> bool {
                let Some(g) = gui.upgrade() else { return false };
                let r = g.borrow_mut().run(name, args);
                match r {
                    Ok(_) => {
                        state.borrow_mut().0 += 1;
                        refresh();
                        true
                    }
                    Err(e) => {
                        warn(&dialog, &e);
                        false
                    }
                }
            })
        };
        let report = {
            let state = state.clone();
            move || state.borrow().1.clone()
        };
        let report = std::rc::Rc::new(report);
        {
            let apply = apply.clone();
            code_asce.on_click(move |_| {
                apply("structural_code.set", json!({ "template": "asce7" }));
            });
        }
        {
            let apply = apply.clone();
            code_en.on_click(move |_| {
                apply("structural_code.set", json!({ "template": "en1990" }));
            });
        }
        {
            let apply = apply.clone();
            add_nature.on_click(move |_| {
                let types = vec!["permanent".to_owned(), "variable".to_owned(), "extraordinary".to_owned()];
                let Some(a) = form(&dialog, "Add Load Nature", vec![
                    Field::Text("Name", String::new()),
                    Field::Text("Symbol", String::new()),
                    Field::Choice("Action type", types.clone(), 1),
                    Field::Text("IFC action source", "USERDEFINED".into()),
                ]) else {
                    return;
                };
                apply(
                    "load_nature.create",
                    json!({ "name": a[0].text(), "symbol": a[1].text(), "action_type": types[a[2].choice()], "action_source": a[3].text() }),
                );
            });
        }
        let nature_names = {
            let report = report.clone();
            move || -> Vec<String> {
                report()["natures"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n["name"].as_str().map(str::to_owned))
                    .collect()
            }
        };
        let nature_names = std::rc::Rc::new(nature_names);
        {
            let (apply, nature_names) = (apply.clone(), nature_names.clone());
            add_case.on_click(move |_| {
                let names = nature_names();
                let Some(a) = form(&dialog, "Add Load Case", vec![
                    Field::Text("Name", String::new()),
                    Field::Choice("Nature", names.clone(), 0),
                    Field::Text("Category", String::new()),
                    Field::Check("Self-weight", false),
                ]) else {
                    return;
                };
                apply(
                    "load_case.create",
                    json!({ "name": a[0].text(), "nature": names.get(a[1].choice()).cloned().unwrap_or_default(), "category": a[2].text(), "self_weight": a[3].check() }),
                );
            });
        }
        {
            let (apply, nature_names, report) = (apply.clone(), nature_names.clone(), report.clone());
            edit_case.on_click(move |_| {
                let Some(i) = selected(&cases) else { return };
                let c = report()["cases"][i].clone();
                let names = nature_names();
                let at = names
                    .iter()
                    .position(|n| c["nature"] == n.as_str())
                    .unwrap_or(0);
                let Some(a) = form(&dialog, "Edit Load Case", vec![
                    Field::Text("Name", c["name"].as_str().unwrap_or_default().into()),
                    Field::Choice("Nature", names.clone(), at),
                    Field::Text("Category", c["category"].as_str().unwrap_or_default().into()),
                    Field::Check("Self-weight", c["self_weight"] == true),
                ]) else {
                    return;
                };
                apply(
                    "load_case.set",
                    json!({ "case_id": c["id"], "name": a[0].text(), "nature": names.get(a[1].choice()).cloned().unwrap_or_default(), "category": a[2].text(), "self_weight": a[3].check() }),
                );
            });
        }
        {
            let (apply, report) = (apply.clone(), report.clone());
            delete_case.on_click(move |_| {
                let Some(i) = selected(&cases) else { return };
                let r = report();
                let c = r["cases"][i].clone();
                let name = c["name"].as_str().unwrap_or_default().to_owned();
                let used = r["combinations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|k| {
                        k["formula"]
                            .as_str()
                            .unwrap_or_default()
                            .split(['+', '-', '*', ' '])
                            .any(|t| t.trim() == name)
                    })
                    .count();
                let d = MessageDialog::builder(
                    &dialog,
                    &format!(
                        "Delete load case {name}? Its {} load(s) are deleted and it is taken out of {used} combination(s).",
                        c["loads"]
                    ),
                    "Delete Load Case",
                )
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning)
                .build();
                if d.show_modal() == ID_YES {
                    apply("load_case.delete", json!({ "case_id": c["id"] }));
                }
            });
        }
        let combo_form = move |title: &str, c: &Value| {
            let ls: Vec<String> = LIMIT_STATES.iter().map(|s| s.to_string()).collect();
            let at = LIMIT_STATES
                .iter()
                .position(|s| c["limit_state"] == *s)
                .unwrap_or(0);
            let u: Vec<String> = c["usages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect();
            form(&dialog, title, vec![
                Field::Text("Name", c["name"].as_str().unwrap_or_default().into()),
                Field::Text("Formula (e.g. 1.2*DL + 1.6*LL)", c["formula"].as_str().unwrap_or_default().into()),
                Field::Choice("Limit state", ls, at),
                Field::Text("Usages (comma separated)", u.join(", ")),
            ])
            .map(|a| {
                (
                    a[0].text(),
                    a[1].text(),
                    LIMIT_STATES[a[2].choice()].to_owned(),
                    a[3].text()
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>(),
                )
            })
        };
        let combo_form = std::rc::Rc::new(combo_form);
        {
            let (apply, combo_form) = (apply.clone(), combo_form.clone());
            add_combo.on_click(move |_| {
                let Some((name, formula, ls, usages)) = combo_form("Add Load Combination", &Value::Null) else {
                    return;
                };
                apply(
                    "load_combination.create",
                    json!({ "name": name, "formula": formula, "limit_state": ls, "usages": usages }),
                );
            });
        }
        {
            let (apply, combo_form, report) = (apply.clone(), combo_form.clone(), report.clone());
            edit_combo.on_click(move |_| {
                let Some(i) = selected(&combos) else { return };
                let c = report()["combinations"][i].clone();
                let Some((name, formula, ls, usages)) = combo_form("Edit Load Combination", &c) else {
                    return;
                };
                apply(
                    "load_combination.set",
                    json!({ "combination_id": c["id"], "name": name, "formula": formula, "limit_state": ls, "usages": usages }),
                );
            });
        }
        {
            let (apply, report) = (apply.clone(), report.clone());
            delete_combo.on_click(move |_| {
                let Some(i) = selected(&combos) else { return };
                let c = report()["combinations"][i].clone();
                apply("load_combination.delete", json!({ "combination_id": c["id"] }));
            });
        }
        {
            let (apply, gui) = (apply.clone(), gui.clone());
            generate.on_click(move |_| {
                let Some(g) = gui.upgrade() else { return };
                let r = g.borrow_mut().run("load_combination.generate", json!({}));
                let r = match r {
                    Ok(r) => r,
                    Err(e) => return warn(&dialog, &e),
                };
                let props = r["proposals"].as_array().cloned().unwrap_or_default();
                if props.is_empty() {
                    return warn(&dialog, "The template proposes no combinations for these cases.");
                }
                let labels: Vec<String> = props
                    .iter()
                    .map(|p| format!("{} = {} ({})", p["name"].as_str().unwrap_or_default(), p["formula"].as_str().unwrap_or_default(), p["limit_state"].as_str().unwrap_or_default()))
                    .collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let flagged: Vec<String> = r["flagged"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect();
                let message = if flagged.is_empty() {
                    "Add the chosen combinations to the table:".to_owned()
                } else {
                    format!("Add the chosen combinations (cases the template does not know: {}):", flagged.join(", "))
                };
                let d = MultiChoiceDialog::builder(&dialog, &message, "Generate Combinations", &refs).build();
                d.set_selections(&(0..refs.len() as i32).collect::<Vec<_>>());
                if d.show_modal() != ID_OK {
                    return;
                }
                for k in d.get_selections() {
                    let p = &props[k as usize];
                    apply(
                        "load_combination.create",
                        json!({ "name": p["name"], "formula": p["formula"], "limit_state": p["limit_state"] }),
                    );
                }
            });
        }
        // Usages: renaming or deleting a tag changes every combination that has it.
        let retag = {
            let (apply, report) = (apply.clone(), report.clone());
            move |from: &str, to: Option<&str>| {
                for c in report()["combinations"].as_array().cloned().unwrap_or_default() {
                    let u: Vec<String> = c["usages"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect();
                    if !u.iter().any(|x| x == from) {
                        continue;
                    }
                    let next: Vec<String> = u
                        .into_iter()
                        .filter_map(|x| if x == from { to.map(str::to_owned) } else { Some(x) })
                        .collect();
                    apply("load_combination.set", json!({ "combination_id": c["id"], "usages": next }));
                }
            }
        };
        let retag = std::rc::Rc::new(retag);
        {
            let retag = retag.clone();
            rename_usage.on_click(move |_| {
                let Some(i) = selected(&usages) else { return };
                let from = usages.get_item_text(i as i64, 0);
                let d = TextEntryDialog::builder(&dialog, "New name:", "Rename Usage")
                    .with_default_value(&from)
                    .build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let to = d.get_value().unwrap_or_default();
                if !to.trim().is_empty() {
                    retag(&from, Some(to.trim()));
                }
            });
        }
        {
            let retag = retag.clone();
            delete_usage.on_click(move |_| {
                let Some(i) = selected(&usages) else { return };
                let from = usages.get_item_text(i as i64, 0);
                retag(&from, None);
            });
        }
        let ok_pressed = dialog.show_modal() == ID_OK;
        let made = state.borrow().0;
        dialog.destroy();
        let mut g = gui_rc.borrow_mut();
        if !ok_pressed && made > 0 {
            let _ = g.run("edit.undo", json!({ "steps": made }));
        }
        g.refresh_scene(&frame);
        frame.set_status_text(
            &if ok_pressed {
                format!("Load cases and combinations: {made} change(s)")
            } else {
                "Load cases and combinations unchanged".to_owned()
            },
            0,
        );
    }

    /// The load cases' names and ids, or a message when there are none.
    fn cases(&mut self) -> Result<Vec<(String, Value)>, String> {
        let r = self.run("load_case.list", json!({}))?;
        let out: Vec<(String, Value)> = r["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (c["name"].as_str().unwrap_or_default().to_owned(), c["id"].clone()))
            .collect();
        if out.is_empty() {
            return Err("Make a load case first (Analyze › Loads › Load Cases)".into());
        }
        Ok(out)
    }

    /// Analyze › Loads › Point Load, Line Load, Area Load or Point Support: asks for the values,
    /// then places on what is clicked in a plan.
    pub(crate) fn start_load_tool(&mut self, which: &str) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Loads are placed in plan views", 0);
        };
        let kind = if which == "support" {
            let states = vec!["pinned".to_owned(), "fixed".to_owned(), "roller".to_owned()];
            let Some(a) = form(&frame, "Point Support", vec![Field::Choice("State", states.clone(), 0)]) else {
                return;
            };
            LoadToolKind::Support { state: states[a[0].choice()].clone() }
        } else {
            let cases = match self.cases() {
                Ok(c) => c,
                Err(e) => return frame.set_status_text(&e, 0),
            };
            let names: Vec<String> = cases.iter().map(|c| c.0.clone()).collect();
            match which {
                "point" => {
                    let Some(a) = form(&frame, "Point Load", vec![
                        Field::Choice("Load case", names, 0),
                        Field::Text("Fx (kN)", "0".into()),
                        Field::Text("Fy (kN)", "0".into()),
                        Field::Text("Fz (kN)", "-10".into()),
                    ]) else {
                        return;
                    };
                    LoadToolKind::Point {
                        case: cases[a[0].choice()].1.clone(),
                        forces: [a[1].number() * 1e3, a[2].number() * 1e3, a[3].number() * 1e3, 0.0, 0.0, 0.0],
                    }
                }
                "line" => {
                    let Some(a) = form(&frame, "Line Load", vec![
                        Field::Choice("Load case", names, 0),
                        Field::Text("Start wz (kN/m)", "-5".into()),
                        Field::Text("End wz (kN/m, blank: uniform)", String::new()),
                        Field::Text("wx (kN/m)", "0".into()),
                        Field::Text("wy (kN/m)", "0".into()),
                    ]) else {
                        return;
                    };
                    let (wx, wy) = (a[3].number() * 1e3, a[4].number() * 1e3);
                    let s = a[1].number() * 1e3;
                    let e = if a[2].text().is_empty() { s } else { a[2].number() * 1e3 };
                    LoadToolKind::Line {
                        case: cases[a[0].choice()].1.clone(),
                        start: [wx, wy, s],
                        end: [wx, wy, e],
                    }
                }
                _ => {
                    let Some(a) = form(&frame, "Area Load", vec![
                        Field::Choice("Load case", names, 0),
                        Field::Text("qz (kPa = kN/m²)", "-2.4".into()),
                    ]) else {
                        return;
                    };
                    LoadToolKind::Area {
                        case: cases[a[0].choice()].1.clone(),
                        value: [0.0, 0.0, a[1].number() * 1e3],
                    }
                }
            }
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        if !self.show_analytical {
            self.toggle_analytical();
        }
        frame.set_status_text(
            match &kind {
                LoadToolKind::Point { .. } => "Point Load: click near a node or along a member at this level. Esc ends the tool.",
                LoadToolKind::Line { .. } => "Line Load: click the members to load at this level. Esc ends the tool.",
                LoadToolKind::Area { .. } => "Area Load: click inside the panels to load at this level. Esc ends the tool.",
                LoadToolKind::Support { .. } => "Point Support: click near the nodes to support at this level. Esc ends the tool.",
            },
            0,
        );
        self.load_tool = Some(LoadTool { level, kind, placed: 0 });
    }

    pub(crate) fn end_load_tool(&mut self) -> bool {
        self.load_tool.take().is_some()
    }

    /// What a click in a plan at the tool's level is on: the nearest node within 0.4 m, the
    /// nearest member within 0.4 m (and where along it), the panel under it; looking from 1.5 m
    /// below the level to 0.5 m above it.
    fn analytical_at(&self, p: [f64; 2], level: ElementId) -> (Option<ElementId>, Option<(ElementId, f64)>, Option<ElementId>) {
        let Some(store) = self.store_now() else {
            return (None, None, None);
        };
        let z = store.get(level).map(reviera_core::levels::elevation).unwrap_or(0.0);
        let band = |q: f64| q >= z - 1.5 && q <= z + 0.5;
        let reach = 0.4;
        let node = reviera_core::analytical::nodes(&store)
            .into_iter()
            .filter(|(_, q)| band(q[2]))
            .map(|(id, q)| (id, (q[0] - p[0]).hypot(q[1] - p[1])))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let member = reviera_core::analytical::members(&store)
            .into_iter()
            .filter(|(_, m)| band(m.start[2]) && band(m.end[2]))
            .filter_map(|(id, m)| {
                let d = [m.end[0] - m.start[0], m.end[1] - m.start[1]];
                let l2 = d[0] * d[0] + d[1] * d[1];
                if l2 < 1e-9 {
                    return None;
                }
                let t = (((p[0] - m.start[0]) * d[0] + (p[1] - m.start[1]) * d[1]) / l2).clamp(0.0, 1.0);
                let q = [m.start[0] + d[0] * t, m.start[1] + d[1] * t];
                let dist = (q[0] - p[0]).hypot(q[1] - p[1]);
                (dist <= reach).then_some((id, t, dist))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, t, _)| (id, t));
        let panel = reviera_core::analytical_panels::panels(&store)
            .into_iter()
            .filter(|(_, g)| g.normal.is_some_and(|n| n[2].abs() > 0.5))
            .find(|(_, g)| {
                let zc = g.corners.iter().map(|c| c[2]).sum::<f64>() / g.corners.len() as f64;
                band(zc) && g.contains([p[0], p[1], zc], 0.01)
            })
            .map(|(id, _)| id);
        (node, member, panel)
    }

    pub(crate) fn load_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.load_tool.clone() else { return };
        let Some(p) = self.beam_point(x, y, None) else { return };
        let (node, member, panel) = self.analytical_at(p, t.level);
        let result = match &t.kind {
            LoadToolKind::Point { case, forces } => match (node, member) {
                (Some(n), _) => self.run("load.create_point", json!({ "host_id": n, "case_id": case, "forces": forces })),
                (None, Some((m, at))) => self.run(
                    "load.create_point",
                    json!({ "host_id": m, "position": at, "case_id": case, "forces": forces }),
                ),
                _ => Err("No analytical node or member there".into()),
            },
            LoadToolKind::Line { case, start, end } => match member {
                Some((m, _)) => self.run("load.create_line", json!({ "host_id": m, "case_id": case, "start": start, "end": end })),
                None => Err("No analytical member there".into()),
            },
            LoadToolKind::Area { case, value } => match panel {
                Some(pid) => self.run("load.create_area", json!({ "host_id": pid, "case_id": case, "value": value })),
                None => Err("No horizontal analytical panel there at this level".into()),
            },
            LoadToolKind::Support { state } => match node {
                Some(n) => self.run("boundary_condition.create", json!({ "kind": "point", "host_id": n, "state": state })),
                None => Err("No analytical node there".into()),
            },
        };
        match result {
            Ok(_) => {
                if let Some(t) = &mut self.load_tool {
                    t.placed += 1;
                }
                let n = self.load_tool.as_ref().map_or(0, |t| t.placed);
                frame.set_status_text(&format!("{n} placed; click the next, or Esc"), 0);
                self.refresh_scene(&frame);
                for i in 0..self.views.len() {
                    self.update_inspection(i);
                }
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
