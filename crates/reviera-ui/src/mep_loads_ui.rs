// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › MEP Settings › Load Classifications and Demand Factors (MEP-ELEC-009, 010): the
//! classifications with their demand factor, space-load flag and how many connectors use them
//! (New, Edit, Delete asking for a replacement while connectors use it); the demand factors
//! (constant, by quantity or by load, ranges at percentages, incremental, the largest-motor
//! rule), entered as text such as `0-10000: 100; 10000-: 50`.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const NEW: i32 = 7201;
const EDIT: i32 = 7202;
const DELETE: i32 = 7203;
const FACTORS: i32 = 7204;

/// A list in a dialog with buttons that close it: which one, and the selected row.
fn list_dialog(frame: &Frame, title: &str, intro: &str, columns: &[(&str, i32)], rows: &[Vec<String>], buttons: &[(i32, &str, &str)], selected: Option<usize>) -> (i32, Option<usize>) {
    let dialog = Dialog::builder(frame, title).with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder).build();
    let text = StaticText::builder(&dialog).with_label(intro).build();
    text.wrap(560);
    let list = ListCtrl::builder(&dialog).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
    for (k, (h, w)) in columns.iter().enumerate() {
        list.insert_column(k as i64, h, ListColumnFormat::Left, *w);
    }
    for (k, r) in rows.iter().enumerate() {
        list.insert_item(k as i64, r.first().map_or("", String::as_str), None);
        for (c, t) in r.iter().enumerate().skip(1) {
            list.set_item_text_by_column(k as i64, c as i32, t);
        }
    }
    list.set_min_size(Size::new(columns.iter().map(|c| c.1).sum::<i32>() + 24, 240));
    if let Some(k) = selected.filter(|k| *k < rows.len()).or((!rows.is_empty()).then_some(0)) {
        list.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
    }
    list.on_item_activated(move |_| dialog.end_modal(EDIT));
    let side = BoxSizer::builder(Orientation::Vertical).build();
    for (code, label, tip) in buttons {
        let b = Button::builder(&dialog).with_label(label).build();
        b.set_tooltip(tip);
        let code = *code;
        b.on_click(move |_| dialog.end_modal(code));
        side.add(&b, 0, SizerFlag::Expand | SizerFlag::Bottom, 4);
    }
    side.add_stretch_spacer(1);
    let close = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
    side.add(&close, 0, SizerFlag::Expand, 0);
    let left = BoxSizer::builder(Orientation::Vertical).build();
    left.add(&text, 0, SizerFlag::Expand | SizerFlag::Bottom, 8);
    left.add(&list, 1, SizerFlag::Expand, 0);
    let sizer = BoxSizer::builder(Orientation::Horizontal).build();
    sizer.add_sizer(&left, 1, SizerFlag::Expand | SizerFlag::All, 12);
    sizer.add_sizer(&side, 0, SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom | SizerFlag::Right, 12);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_escape_id(ID_CANCEL);
    let code = dialog.show_modal();
    let sel = usize::try_from(list.get_first_selected_item()).ok();
    dialog.destroy();
    (code, sel)
}

fn method_label(m: &str) -> &str {
    match m {
        "constant" => "Constant",
        "quantity" => "By quantity",
        "load" => "By load",
        other => other,
    }
}

/// Ranges as text: `0-10000: 100; 10000-: 50` (a constant factor: `80`).
fn ranges_text(d: &Value) -> String {
    let rs = d["ranges"].as_array().cloned().unwrap_or_default();
    if d["method"] == "constant" {
        return rs.first().and_then(|r| r["percent"].as_f64()).map_or_else(|| "100".into(), |p| format!("{p}"));
    }
    rs.iter()
        .map(|r| {
            let to = r["to"].as_f64().map_or_else(String::new, |t| format!("{t}"));
            format!("{}-{to}: {}", r["from"].as_f64().unwrap_or(0.0), r["percent"].as_f64().unwrap_or(100.0))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Parses ranges text (see [`ranges_text`]).
fn parse_ranges(text: &str, constant: bool) -> Result<Value, String> {
    let num = |s: &str| s.trim().trim_end_matches('%').trim().parse::<f64>().map_err(|_| format!("{s:?} is not a number"));
    if constant {
        return Ok(json!([{ "from": 0.0, "percent": num(text)? }]));
    }
    let mut out = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (range, pct) = part.split_once(':').ok_or_else(|| format!("{part:?}: write a range and its percentage, as 0-10000: 100"))?;
        let (from, to) = range.split_once('-').ok_or_else(|| format!("{range:?}: write a range as from-to (to may be empty)"))?;
        let mut r = json!({ "from": num(from)?, "percent": num(pct)? });
        if !to.trim().is_empty() {
            r["to"] = json!(num(to)?);
        }
        out.push(r);
    }
    if out.is_empty() {
        return Err("give at least one range".into());
    }
    Ok(Value::Array(out))
}

impl Gui {
    /// Manage › MEP Settings › Load Classifications.
    pub(crate) fn load_classifications(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("load_classification.list", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|c| {
                    vec![
                        c["name"].as_str().unwrap_or("").to_owned(),
                        c["demand_factor_name"].as_str().unwrap_or("").to_owned(),
                        if c["space_load"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
                        c["connectors"].to_string(),
                    ]
                })
                .collect();
            let (code, at) = list_dialog(
                &frame,
                "Load Classifications",
                "Loads are grouped by classification in panel schedules, each with its demand factor. Connectors name their classification; an unknown name counts as Other. The five defaults can be renamed but not deleted.",
                &[("Classification", 180), ("Demand factor", 160), ("Space load", 80), ("Connectors", 80)],
                &rows,
                &[
                    (NEW, "New…", "Add a load classification"),
                    (EDIT, "Edit…", "Rename it, or change its demand factor or space-load flag"),
                    (DELETE, "Delete…", "Delete it; connectors using it take another"),
                    (FACTORS, "Demand Factors…", "Make and edit demand factors"),
                ],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| list.get(k)).cloned();
            match (code, chosen) {
                (NEW, _) => self.classification_form(None),
                (EDIT, Some(c)) => self.classification_form(Some(&c)),
                (DELETE, Some(c)) => {
                    let name = c["name"].as_str().unwrap_or("").to_owned();
                    let mut args = json!({ "id": c["id"] });
                    if c["connectors"].as_u64().unwrap_or(0) > 0 {
                        let others: Vec<&Value> = list.iter().filter(|o| o["id"] != c["id"]).collect();
                        let labels: Vec<&str> = others.iter().map(|o| o["name"].as_str().unwrap_or("")).collect();
                        let d = SingleChoiceDialog::builder(&frame, &format!("{} connector(s) use {name}. Give them:", c["connectors"]), "Delete Load Classification", &labels).build();
                        if let Some(k) = others.iter().position(|o| o["id"] == "other") {
                            d.set_selection(k as i32);
                        }
                        if d.show_modal() != ID_OK {
                            continue;
                        }
                        let Some(o) = usize::try_from(d.get_selection()).ok().and_then(|k| others.get(k)) else { continue };
                        args["replace_with"] = o["id"].clone();
                    }
                    if let Err(e) = self.run("load_classification.delete", args) {
                        error(&frame, &format!("{name} was not deleted:\n{e}"));
                    }
                    self.refresh_active();
                    self.refresh_schedules();
                }
                (FACTORS, _) => self.demand_factors(),
                (EDIT | DELETE, None) => {}
                _ => break,
            }
        }
    }

    fn demand_factor_names(&mut self) -> Vec<(String, String)> {
        self.run("electrical_settings.get", json!({}))
            .ok()
            .and_then(|s| s["demand_factors"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|d| (d["id"].as_str().unwrap_or("").to_owned(), d["name"].as_str().unwrap_or("").to_owned()))
            .collect()
    }

    fn classification_form(&mut self, existing: Option<&Value>) {
        let frame = self.frame;
        let factors = self.demand_factor_names();
        let names: Vec<String> = factors.iter().map(|f| f.1.clone()).collect();
        let current = existing.and_then(|c| c["demand_factor"].as_str()).unwrap_or("constant_100");
        let f = super::reinforcement_ui::Form::new(&frame, if existing.is_some() { "Edit Load Classification" } else { "New Load Classification" });
        let name = f.text("Name", existing.and_then(|c| c["name"].as_str()).unwrap_or(""), "1 to 64 characters, unique");
        let df = f.choice("Demand factor", &names, factors.iter().position(|x| x.0 == current).unwrap_or(0));
        let space = f.check("Space load", "Counts towards spaces' electrical load densities", existing.is_some_and(|c| c["space_load"].as_bool() == Some(true)));
        let ok = f.show("Renaming also renames it on every connector that names it.", "OK");
        let df_id = df.get_selection().and_then(|k| factors.get(k as usize)).map(|x| x.0.clone());
        let (n, sl) = (name.get_value(), space.get_value());
        f.dialog.destroy();
        if !ok {
            return;
        }
        let r = match existing {
            None => self.run("load_classification.create", json!({ "name": n, "demand_factor_id": df_id, "space_load": sl })).map(|_| ()),
            Some(c) => self.run("load_classification.set", json!({ "id": c["id"], "name": n, "demand_factor_id": df_id, "space_load": sl })).map(|_| ()),
        };
        match r {
            Ok(()) => self.refresh_active(),
            Err(e) => error(&frame, &format!("The load classification was not saved:\n{e}")),
        }
    }

    /// The Demand Factors dialog.
    pub(crate) fn demand_factors(&mut self) {
        let frame = self.frame;
        let mut selected = None;
        loop {
            let list = match self.run("electrical_settings.get", json!({})) {
                Ok(v) => v["demand_factors"].as_array().cloned().unwrap_or_default(),
                Err(e) => return error(&frame, &e),
            };
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|d| {
                    vec![
                        d["name"].as_str().unwrap_or("").to_owned(),
                        method_label(d["method"].as_str().unwrap_or("")).to_owned(),
                        ranges_text(d),
                        if d["incremental"].as_bool() == Some(true) { "Yes".into() } else { String::new() },
                    ]
                })
                .collect();
            let (code, at) = list_dialog(
                &frame,
                "Demand Factors",
                "How much of a classification's connected load is expected at once. Ranges are counts (by quantity) or VA (by load); incremental applies each range's percentage to the part in it.",
                &[("Demand factor", 160), ("Method", 100), ("Ranges (from-to: %)", 220), ("Incremental", 80)],
                &rows,
                &[(NEW, "New…", "Add a demand factor"), (EDIT, "Edit…", "Change it"), (DELETE, "Delete", "Delete a demand factor no classification uses")],
                selected,
            );
            selected = at;
            let chosen = at.and_then(|k| list.get(k)).cloned();
            match (code, chosen) {
                (NEW, _) => self.demand_factor_form(None),
                (EDIT, Some(d)) => self.demand_factor_form(Some(&d)),
                (DELETE, Some(d)) => {
                    if let Err(e) = self.run("demand_factor.delete", json!({ "id": d["id"] })) {
                        error(&frame, &format!("The demand factor was not deleted:\n{e}"));
                    }
                    self.refresh_schedules();
                }
                (EDIT | DELETE, None) => {}
                _ => break,
            }
        }
    }

    fn demand_factor_form(&mut self, existing: Option<&Value>) {
        let frame = self.frame;
        let methods = [("constant", "Constant"), ("quantity", "By quantity (number of loads)"), ("load", "By load (total VA)")];
        let (mut name, mut method, mut ranges, mut inc, mut motors) = match existing {
            Some(d) => (
                d["name"].as_str().unwrap_or("").to_owned(),
                methods.iter().position(|m| d["method"] == m.0).unwrap_or(0),
                ranges_text(d),
                d["incremental"].as_bool().unwrap_or(false),
                d["motor_percents"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|p| p.to_string()).collect::<Vec<_>>().join(", "),
            ),
            None => (String::new(), 0, "100".to_owned(), false, String::new()),
        };
        loop {
            let f = super::reinforcement_ui::Form::new(&frame, if existing.is_some() { "Edit Demand Factor" } else { "New Demand Factor" });
            let n = f.text("Name", &name, "");
            let m = f.choice("Method", &methods.iter().map(|m| m.1.to_owned()).collect::<Vec<_>>(), method);
            let r = f.text("Ranges", &ranges, "Constant: one percentage (80). Otherwise from-to: percent; …, for example 0-10000: 100; 10000-: 50 (an empty end has no limit)");
            let i = f.check("Incremental", "Each range's percentage on the part that falls in it", inc);
            let mo = f.text("Largest motor rule", &motors, "Optional: percentages for motor loads, largest first (for example 125, 100, 75); empty when off");
            let ok = f.show("", "OK");
            (name, method, ranges, inc, motors) = (n.get_value(), m.get_selection().map_or(0, |k| k as usize), r.get_value(), i.get_value(), mo.get_value());
            f.dialog.destroy();
            if !ok {
                return;
            }
            let parsed = parse_ranges(&ranges, method == 0).and_then(|rs| {
                let mp: Result<Vec<f64>, String> =
                    motors.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| s.trim_end_matches('%').trim().parse::<f64>().map_err(|_| format!("{s:?} is not a percentage"))).collect();
                Ok((rs, mp?))
            });
            let (rs, mp) = match parsed {
                Ok(x) => x,
                Err(e) => {
                    error(&frame, &e);
                    continue;
                }
            };
            let body = json!({ "name": name, "method": methods[method.min(2)].0, "ranges": rs, "incremental": inc, "motor_percents": mp });
            let result = match existing {
                None => self.run("demand_factor.create", body).map(|_| ()),
                Some(d) => {
                    let mut b = body;
                    b["id"] = d["id"].clone();
                    self.run("demand_factor.set", b).map(|_| ())
                }
            };
            match result {
                Ok(()) => {
                    self.refresh_active();
                    self.refresh_schedules();
                    return;
                }
                Err(e) => error(&frame, &format!("The demand factor was not saved:\n{e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_round_trip() {
        let r = parse_ranges("0-10000: 100; 10000-: 50", false).unwrap();
        assert_eq!(r, json!([{ "from": 0.0, "to": 10000.0, "percent": 100.0 }, { "from": 10000.0, "percent": 50.0 }]));
        assert_eq!(ranges_text(&json!({ "method": "load", "ranges": r })), "0-10000: 100; 10000-: 50");
        assert_eq!(parse_ranges("80 %", true).unwrap(), json!([{ "from": 0.0, "percent": 80.0 }]));
        assert!(parse_ranges("lots", false).is_err());
    }
}
