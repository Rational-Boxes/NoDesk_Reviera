// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Check Bathrooms (ARC-BATH-002): runs `bath.check` under the bathroom rule packs in use, lists
//! the findings (errors first), and selects and frames the fixture and what is in its way. Place
//! Grab Bars (ARC-BATH-003) puts the bars the packs ask for at the selected fixtures, or all.

use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    pub(crate) fn check_bathrooms(&mut self) {
        let frame = self.frame;
        let in_use: Vec<String> = self
            .run("bath.rule_packs", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|p| p["in_use"] == true)
            .filter_map(|p| p["title"].as_str().map(str::to_owned))
            .collect();
        if in_use.is_empty() {
            return error(
                &frame,
                "No bathroom rule packs are in use. Choose them in Manage › Model Properties, or set a building code or accessibility rule set they follow.",
            );
        }
        let found = match self.run("bath.check", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        let packs = in_use.join("; ");
        if found.is_empty() {
            let d = MessageDialog::builder(&frame, &format!("No problems found under: {packs}."), "Check Bathrooms")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let severity = if f["severity"] == "note" { "Note" } else { "Error" };
                let mut row = format!("{}. {severity}: {}", k + 1, f["message"].as_str().unwrap_or_default());
                if f["confidence"] != "verified" {
                    row.push_str(&format!(" [{}]", f["confidence"].as_str().unwrap_or_default()));
                }
                row
            })
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let errors = found.iter().filter(|f| f["severity"] == "error").count();
        let dialog = SingleChoiceDialog::builder(
            &frame,
            &format!(
                "{errors} error(s) and {} note(s) under: {packs}.\nPick one to select and frame it:",
                found.len() - errors
            ),
            "Check Bathrooms",
            &labels,
        )
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(f) = picked.and_then(|i| found.get(i)) else {
            return;
        };
        // Show the clear spaces, then the fixture and what is in its way.
        if let Some(v) = self.active_view()
            && self.virtual_mode(v) == reviera_render::VirtualMode::Hidden
            && let Some(view) = v.element
        {
            let _ = self.run("view.set_virtual_volumes", json!({ "view_id": view, "mode": "translucent" }));
            self.refresh_scene(&frame);
        }
        let mut ids = vec![f["element"].clone()];
        if !f["other"].is_null() {
            ids.push(f["other"].clone());
        }
        let _ = self.run("selection.set", json!({ "ids": ids }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(
            &format!(
                "{} — {} ({}, {})",
                f["rule"].as_str().unwrap_or_default(),
                f["description"].as_str().unwrap_or_default(),
                f["edition"].as_str().unwrap_or_default(),
                f["source"].as_str().unwrap_or_default()
            ),
            0,
        );
    }

    pub(crate) fn place_grab_bars(&mut self) {
        let frame = self.frame;
        let selected: Vec<serde_json::Value> = self.selection().into_iter().map(|id| json!(id.0)).collect();
        let args = if selected.is_empty() { json!({}) } else { json!({ "element_ids": selected }) };
        let out = match self.run("bath.place_grab_bars", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Grab bars were not placed:\n{e}")),
        };
        self.refresh_scene(&frame);
        let placed = out["placed"].as_array().map_or(0, Vec::len);
        let skipped: Vec<String> = out["skipped"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|s| format!("{} ({}): {}", self.element_label(&s["fixture"]), s["rule"].as_str().unwrap_or_default(), s["reason"].as_str().unwrap_or_default()))
            .collect();
        if placed == 0 && skipped.is_empty() {
            return error(&frame, "No grab bars to place: the bathroom rule packs in use ask for none at these fixtures (the ADA pack places WC bars).");
        }
        if skipped.is_empty() {
            frame.set_status_text(&format!("{placed} grab bar(s) placed"), 0);
        } else {
            let d = MessageDialog::builder(&frame, &format!("{placed} grab bar(s) placed. Not placed:\n{}", skipped.join("\n")), "Place Grab Bars")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
        }
    }

    /// Architecture › Build › Bathroom (ARC-BATH-004): a generic bathroom fixture or accessory,
    /// loaded the first time; each click near a wall puts one on its face at its own height.
    pub(crate) fn place_bathroom_component(&mut self) {
        let frame = self.frame;
        let components = |store: &reviera_core::Store| -> Vec<(reviera_core::ElementId, String)> {
            store.iter().filter(|e| reviera_core::bath_components::is_component(e)).map(|e| (e.id(), e.name.clone().unwrap_or_default())).collect()
        };
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut types = components(&store);
        if types.len() < reviera_core::bath_components::names().len() {
            if let Err(e) = self.run("bath.load_components", json!({})) {
                return error(&frame, &e);
            }
            let Some(store) = self.store_now() else { return };
            types = components(&store);
        }
        // In the catalogue's order: fixtures, then accessories.
        let order = reviera_core::bath_components::names();
        types.sort_by_key(|t| order.iter().position(|n| *n == t.1).unwrap_or(usize::MAX));
        let f = super::reinforcement_ui::Form::new(&frame, "Place Bathroom Component");
        let names: Vec<String> = types.iter().map(|t| t.1.clone()).collect();
        let ty = f.choice("Component", &names, 0);
        let height = f.text("Raise by (mm)", "0", "Above its own mounting height");
        let ok = f.show("Click near a wall to put it on that face, its back to the wall (it follows the wall); away from walls it stands on the work plane. Spacebar turns it there.", "Place");
        let k = ty.get_selection().map_or(0, |k| k as usize);
        let h = height.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(0.0) / 1000.0;
        f.dialog.destroy();
        let Some((id, name)) = types.get(k).cloned().filter(|_| ok) else { return };
        self.start_place_tool(super::components::Source::Type(id), name);
        self.set_place_offset(h);
    }

    /// Architecture › Build › Bathroom Layout (ARC-BATH-005): a template and its options, then a
    /// click inside a room closed by walls lays it out there.
    pub(crate) fn bathroom_layout(&mut self) {
        let frame = self.frame;
        let templates = match self.run("bath.layouts", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Bathroom Layout");
        let titles: Vec<String> = templates.iter().map(|t| t["title"].as_str().unwrap_or_default().to_owned()).collect();
        let t = f.choice("Template", &titles, 0);
        let start = f.choice("Start from", &["The left end, facing the wet wall".to_owned(), "The right end".to_owned()], 0);
        let replace = f.choice("An earlier layout here", &["Replace it".to_owned(), "Keep it".to_owned()], 0);
        let ok = f.show("Then click inside a room closed by walls, in a plan: the fixtures run along its longer wall, spaced by the bathroom rule packs in use.", "Choose Room");
        let k = t.get_selection().map_or(0, |k| k as usize);
        let right = start.get_selection() == Some(1);
        let keep = replace.get_selection() == Some(1);
        f.dialog.destroy();
        let Some(id) = templates.get(k).and_then(|t| t["id"].as_str()).filter(|_| ok).map(str::to_owned) else { return };
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(json!({ "template": id, "start": if right { "right" } else { "left" }, "replace": !keep }));
            frame.set_status_text("Bathroom Layout: click inside the room; Esc ends", 0);
        }
    }

    /// Lays out a bathroom where the Bathroom Layout tool was clicked, and reports the fit.
    pub(crate) fn layout_bathroom_at(&mut self, mut args: serde_json::Value) {
        let frame = self.frame;
        // The Locker Room, Sauna and Steam Room tools ride on the same click, with their own
        // commands.
        let command = args.as_object_mut().and_then(|o| o.remove("command")).and_then(|c| c.as_str().map(str::to_owned)).unwrap_or_else(|| "bath.layout".to_owned());
        match self.app.execute(&command, args, &super::user()) {
            Ok(out) => {
                let placed = out.result["placed"].as_array().map_or(0, Vec::len);
                self.refresh_scene(&frame);
                let problems = self
                    .run("bath.check", json!({ "element_ids": out.result["placed"] }))
                    .ok()
                    .and_then(|v| v.as_array().map(|a| a.iter().filter(|f| f["severity"] == "error").count()))
                    .unwrap_or(0);
                let heater = out.result["heater_kw"]
                    .as_f64()
                    .map(|k| format!(", the heater {k} kW for {:.1} m³", out.result["sizing"]["design_volume"].as_f64().unwrap_or(0.0)))
                    .or_else(|| out.result["design_volume"].as_f64().map(|v| format!(", a steam generator for {v:.1} m³")))
                    .unwrap_or_default();
                frame.set_status_text(&format!("{placed} component(s) laid out{heater}; Check Bathrooms finds {problems} error(s)"), 0);
            }
            Err(e) => error(&frame, &format!("The layout was not made:\n{}", e.message)),
        }
    }

    /// Architecture › Make Product (ARC-BATH-006): a manufacturer's model file made into a product
    /// type of a generic component's kind, with its manufacturer data.
    pub(crate) fn make_product(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        if !store.iter().any(|e| reviera_core::bath_components::is_component(e)) {
            if let Err(e) = self.run("bath.load_components", json!({})) {
                return error(&frame, &e);
            }
        }
        let Some(store) = self.store_now() else { return };
        let order = reviera_core::bath_components::names();
        let mut generics: Vec<(reviera_core::ElementId, String)> = store
            .iter()
            .filter(|e| reviera_core::bath_components::is_component(e))
            .filter_map(|e| e.name.clone().filter(|n| order.contains(&n.as_str())).map(|n| (e.id(), n)))
            .collect();
        generics.sort_by_key(|g| order.iter().position(|n| *n == g.1).unwrap_or(usize::MAX));
        let fd = FileDialog::builder(&frame)
            .with_message("The manufacturer's model")
            .with_wildcard("3D models (*.obj;*.stl;*.ply;*.gltf;*.glb)|*.obj;*.stl;*.ply;*.gltf;*.glb")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if fd.show_modal() != ID_OK {
            return;
        }
        let Some(path) = fd.get_path() else { return };
        let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let f = super::reinforcement_ui::Form::new(&frame, "Make Product");
        let names: Vec<String> = generics.iter().map(|g| g.1.clone()).collect();
        let generic = f.choice("In place of", &names, 0);
        let name = f.text("Name", &stem, "The product type's name");
        let turns = ["0°", "90°", "180°", "270°"].map(str::to_owned);
        let turn = f.choice("Turn", &turns, 0);
        let units_list = ["As the file says", "Millimetres", "Centimetres", "Metres", "Inches", "Feet"].map(str::to_owned);
        let units = f.choice("Units", &units_list, 0);
        let manufacturer = f.text("Manufacturer", "", "");
        let model = f.text("Model", "", "Its name in the manufacturer's range");
        let reference = f.text("Model reference", "", "");
        let article = f.text("Article number", "", "");
        let ok = f.show("Its back goes to the wall: turn it until its back faces +y. It is raised to the placeholder's mounting height and takes the placeholder's pipe connections.", "Make");
        let k = generic.get_selection().map_or(0, |k| k as usize);
        let unit = ["auto", "mm", "cm", "m", "in", "ft"][units.get_selection().unwrap_or(0) as usize];
        let args = json!({
            "generic": generics.get(k).map(|g| g.0 .0),
            "path": path,
            "name": name.get_value(),
            "turn": 90.0 * f64::from(turn.get_selection().unwrap_or(0)),
            "units": unit,
            "manufacturer": manufacturer.get_value(),
            "model_label": model.get_value(),
            "model_reference": reference.get_value(),
            "article_number": article.get_value(),
        });
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("bath.make_product", args) {
            Ok(_) => frame.set_status_text("Product made: select placeholders and use Swap for Product, or place it with Bathroom", 0),
            Err(e) => error(&frame, &format!("The product was not made:\n{e}")),
        }
    }

    /// Architecture › Swap for Product (ARC-BATH-006): the selected fixtures get a product type
    /// of their kind; the report says what changed.
    pub(crate) fn swap_for_product(&mut self) {
        let frame = self.frame;
        let selected = self.selection();
        let Some(store) = self.store_now() else { return };
        let category = selected
            .iter()
            .filter_map(|id| store.get(*id))
            .find_map(|e| reviera_core::mesh_objects::type_of(&store, e).map(|(_, t)| t.category));
        let Some(category) = category else {
            return error(&frame, "Select the placeholder fixtures to replace first.");
        };
        let products: Vec<(reviera_core::ElementId, String)> = store
            .iter()
            .filter(|e| reviera_core::bath_components::is_component(e) && reviera_core::mesh_objects::mesh_object_type(e).is_some_and(|t| t.category == category))
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        if products.is_empty() {
            return error(&frame, "No other component of this kind: make one with Make Product first.");
        }
        let names: Vec<String> = products.iter().map(|p| p.1.clone()).collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, &format!("Give the {} selected the type:", selected.len()), "Swap for Product", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let k = usize::try_from(d.get_selection()).unwrap_or(0);
        d.destroy();
        let ids: Vec<u64> = selected.iter().map(|id| id.0).collect();
        let report = match self.run("bath.swap_product", json!({ "element_ids": ids, "type_id": products[k].0 .0 })) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Not swapped:\n{e}")),
        };
        self.refresh_scene(&frame);
        let mut lines = Vec::new();
        for r in report.as_array().cloned().unwrap_or_default() {
            let size = |k: &str| r[k].as_array().map(|v| v.iter().map(|x| format!("{:.0}", x.as_f64().unwrap_or(0.0) * 1000.0)).collect::<Vec<_>>().join(" × ")).unwrap_or_default();
            lines.push(format!("{}: {} mm, was {} mm", self.element_label(&r["element"]), size("size_after"), size("size_before")));
            for m in r["connectors_moved"].as_array().cloned().unwrap_or_default() {
                lines.push(format!("  {} connection moved {:.0} mm", m["name"].as_str().unwrap_or_default(), m["distance"].as_f64().unwrap_or(0.0) * 1000.0));
            }
            for c in r["connectors_lost"].as_array().cloned().unwrap_or_default() {
                lines.push(format!("  {} connection: the product has none", c.as_str().unwrap_or_default()));
            }
            for f in r["findings_new"].as_array().cloned().unwrap_or_default() {
                lines.push(format!("  New: {}", f["message"].as_str().unwrap_or_default()));
            }
            for f in r["findings_gone"].as_array().cloned().unwrap_or_default() {
                lines.push(format!("  Resolved: {}", f["message"].as_str().unwrap_or_default()));
            }
        }
        let d = MessageDialog::builder(&frame, &lines.join("\n"), "Swap for Product").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build();
        d.show_modal();
    }

    /// Fixture Count (ARC-BATH-007): an occupancy and its load, then the fixtures it needs under
    /// the packs in use against those modelled.
    pub(crate) fn fixture_count(&mut self) {
        let frame = self.frame;
        let packs = match self.run("bath.rule_packs", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        // Every count row of the packs, those in use first: (pack, occupancy, label).
        let mut rows: Vec<(String, String, String)> = Vec::new();
        for in_use in [true, false] {
            for p in packs.iter().filter(|p| (p["in_use"] == true) == in_use) {
                for c in p["counts"].as_array().cloned().unwrap_or_default() {
                    let occ = c["occupancy"].as_str().unwrap_or_default().to_owned();
                    rows.push((p["id"].as_str().unwrap_or_default().to_owned(), occ.clone(), format!("{occ} — {} ({})", c["description"].as_str().unwrap_or_default(), p["title"].as_str().unwrap_or_default())));
                }
            }
        }
        if rows.is_empty() {
            return error(&frame, "No bathroom rule pack has fixture counts.");
        }
        let level = self.active_view().and_then(|v| v.plan.clone()).map(|p| p.level);
        let f = super::reinforcement_ui::Form::new(&frame, "Fixture Count");
        let labels: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
        let occ = f.choice("Occupancy", &labels, 0);
        let load = f.text("Occupant load", "100", "People");
        let share = f.text("Males (%)", "50", "The share of the load counted as male");
        let units = f.text("Units", "", "Dwelling or sleeping units, for rows counted per unit");
        let mode = f.choice("Rooms", &["Separate for each sex".to_owned(), "All-gender (IPC 2021 403.1.1)".to_owned()], 0);
        let scope_items = if level.is_some() { vec!["This plan's level".to_owned(), "The whole model".to_owned()] } else { vec!["The whole model".to_owned()] };
        let scope = f.choice("Compare with", &scope_items, 0);
        let ok = f.show("Counts the fixtures the occupancy needs and compares them with those modelled.", "Count");
        let k = occ.get_selection().map_or(0, |k| k as usize);
        let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let mut args = json!({
            "occupancy": rows[k].1, "pack": rows[k].0,
            "occupant_load": num(&load).unwrap_or(0.0),
            "male_share": num(&share).unwrap_or(50.0) / 100.0,
            "all_gender": mode.get_selection() == Some(1),
        });
        if let Some(u) = num(&units) {
            args["units"] = json!(u);
        }
        if let (Some(l), Some(0)) = (level, scope.get_selection()) {
            args["level_id"] = json!(l);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        let c = match self.run("bath.fixture_count", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let mut lines = vec![format!("{} — {} ({})", c["row"].as_str().unwrap_or_default(), c["description"].as_str().unwrap_or_default(), c["source"].as_str().unwrap_or_default()), String::new()];
        for n in c["needs"].as_array().cloned().unwrap_or_default() {
            let name = n["fixture"].as_str().unwrap_or_default().replace('_', " ");
            let split = match (n["male"].as_u64(), n["female"].as_u64()) {
                (Some(m), Some(fe)) => format!(" ({m} male, {fe} female)"),
                (Some(m), None) => format!(" ({m} male)"),
                (None, Some(fe)) => format!(" ({fe} female)"),
                _ => String::new(),
            };
            let short = n["short"].as_u64().unwrap_or(0);
            lines.push(format!("{name}: {} needed{split}, {} modelled{}", n["required"], n["modelled"], if short > 0 { format!(" — {short} short") } else { String::new() }));
        }
        let sub = c["urinals_may_replace"].as_u64().unwrap_or(0);
        if sub > 0 {
            lines.push(format!("\nUrinals may replace up to {sub} of the males' WCs."));
        }
        let d = MessageDialog::builder(&frame, &lines.join("\n"), "Fixture Count").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build();
        d.show_modal();
    }

    /// Toilet Core (ARC-BATH-008): on the selected wall, compartments, urinals and lavatories from
    /// numbers or an occupancy, along its whole face on the plan's side.
    pub(crate) fn toilet_core(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let Some(wall) = self.selection().into_iter().find(|id| store.get(*id).is_some_and(|e| reviera_core::walls::wall_recipe(e).is_some())) else {
            return error(&frame, "Select the wall the toilet core stands against first.");
        };
        let Ok(g) = reviera_core::walls::regenerate(&store, store.get(wall).expect("selected")) else { return };
        let Some(level) = self.active_view().and_then(|v| v.plan.clone()).map(|p| p.level) else {
            return error(&frame, "Open a plan of the core's level first.");
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Toilet Core");
        let side = f.choice("Room side", &["Interior face".to_owned(), "Exterior face".to_owned()], 0);
        let how = f.choice("Numbers", &["Given below".to_owned(), "Counted for an occupancy".to_owned()], 0);
        let wcs = f.text("WCs", "4", "Compartments, the accessible one included");
        let urinals = f.text("Urinals", "2", "");
        let lavatories = f.text("Lavatories", "3", "");
        let occ = f.text("Occupancy", "B", "As the count rules name it (B, A-3, workplace…)");
        let load = f.text("Occupant load", "100", "People in the building or floor");
        let sex = f.choice("Room for", &["Men".to_owned(), "Women".to_owned(), "All genders".to_owned()], 0);
        let acc = f.choice("Accessible compartment", &["Yes".to_owned(), "No".to_owned()], 0);
        let amb = f.choice("Ambulatory compartment", &["No".to_owned(), "Yes".to_owned()], 0);
        let ok = f.show("Lays out along the wall's whole face: an accessible compartment, standard ones, urinals with screens, lavatories. Too short a wall is reported with the shortfall.", "Lay Out");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let s = if side.get_selection() == Some(1) { 1.0 } else { -1.0 };
        let n = g.normal();
        let off = g.width / 2.0 + 0.05;
        let start = [g.start[0] + n[0] * s * off, g.start[1] + n[1] * s * off];
        let end = [g.end[0] + n[0] * s * off, g.end[1] + n[1] * s * off];
        let mut args = json!({ "wall_id": wall.0, "start": start, "end": end, "level": level, "accessible": acc.get_selection() != Some(1), "ambulatory": amb.get_selection() == Some(1), "replace": true });
        if how.get_selection() == Some(1) {
            args["occupancy"] = json!(occ.get_value().trim());
            args["occupant_load"] = json!(num(&load).unwrap_or(0.0));
            args["sex"] = json!(["male", "female", "all"][sex.get_selection().unwrap_or(0) as usize]);
            args["urinals_for_wcs"] = json!(sex.get_selection() == Some(0));
        } else {
            args["wcs"] = json!(num(&wcs).unwrap_or(0.0) as u32);
            args["urinals"] = json!(num(&urinals).unwrap_or(0.0) as u32);
            args["lavatories"] = json!(num(&lavatories).unwrap_or(0.0) as u32);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("bath.toilet_core", args) {
            Ok(c) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Toilet core: {} WC(s), {} urinal(s), {} lavatory(ies)", c["wcs"], c["urinals"], c["lavatories"]), 0);
            }
            Err(e) => error(&frame, &format!("The core was not laid out:\n{e}")),
        }
    }

    /// Architecture › Build › Locker Room (ARC-BATH-009): lockers, tiers, showers, then a click
    /// inside a room closed by walls.
    pub(crate) fn locker_room(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Locker Room");
        let lockers = f.text("Lockers", "40", "Compartments");
        let tiers = f.choice("Lockers", &["Full height".to_owned(), "Two tier".to_owned()], 1);
        let showers = f.text("Gang showers", "6", "Heads along the end wall, 750 mm apart (0 for none)");
        let acc = f.choice("Accessible bench and lockers", &["Yes".to_owned(), "No".to_owned()], 0);
        let ok = f.show("Then click inside a room closed by walls, in a plan: lockers run along its long walls, benches down the middle, showers along one end.", "Choose Room");
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok().unwrap_or(0.0).max(0.0) as u32;
        let args = json!({ "command": "bath.locker_room", "lockers": num(&lockers), "tiers": if tiers.get_selection() == Some(1) { 2 } else { 1 }, "showers": num(&showers), "accessible": acc.get_selection() != Some(1), "replace": true });
        f.dialog.destroy();
        if !ok {
            return;
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Locker Room: click inside the room; Esc ends", 0);
        }
    }

    /// Architecture › Build › Shower Falls (ARC-BATH-011): the fall and the drain, for the
    /// selected level-access showers.
    pub(crate) fn shower_falls(&mut self) {
        let frame = self.frame;
        let selected: Vec<serde_json::Value> = self.selection().into_iter().map(|id| json!(id.0)).collect();
        if selected.is_empty() {
            return error(&frame, "Select level-access showers first: a roll-in shower or a gang shower head standing on a floor.");
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Shower Falls");
        let fall = f.text("Fall (%)", "", "Empty for the rule packs' default (2 % under the IPC, 1:60 under Approved Document M)");
        let drain = f.choice("Drain", &["Centre".to_owned(), "Linear, along the back".to_owned()], 0);
        let ok = f.show("A floor drain is placed and the floor under each shower warps to fall to it.", "Lay Falls");
        let mut args = json!({ "element_ids": selected, "drain": if drain.get_selection() == Some(1) { "linear" } else { "centre" } });
        if let Ok(p) = fall.get_value().trim().trim_end_matches('%').trim().parse::<f64>() {
            args["fall"] = json!(p / 100.0);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("bath.shower_falls", args) {
            Ok(out) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Shower falls: {} drain(s) placed, {} floor(s) warped", out["drains"].as_array().map_or(0, Vec::len), out["floors"].as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("The falls were not laid:\n{e}")),
        }
    }

    /// Architecture › Build › Waterproofing (ARC-BATH-012): the waterproofing schedule for the
    /// selected fixtures or all, offered as CSV.
    pub(crate) fn waterproofing(&mut self) {
        let frame = self.frame;
        let selected: Vec<serde_json::Value> = self.selection().into_iter().map(|id| json!(id.0)).collect();
        let s = match self.run("bath.waterproofing", json!({ "element_ids": selected })) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let extents = s["extents"].as_array().cloned().unwrap_or_default();
        if extents.is_empty() {
            return error(&frame, "No waterproofing: the bathroom rule packs in use ask for none at these fixtures (the IPC pack covers showers, tubs and urinals; Approved Document M wet rooms).");
        }
        let n = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0);
        let mut csv = vec!["Fixture,Kind,Surface,On,Width (m),Height (m),Area (m²),Rule,Class".to_owned()];
        let mut lines = Vec::new();
        for e in &extents {
            let fixture = self.element_label(&e["element"]);
            let on = if e["host"].is_null() { String::new() } else { self.element_label(&e["host"]) };
            let (surface, rule, class) = (e["surface"].as_str().unwrap_or_default(), e["rule"].as_str().unwrap_or_default(), e["class"].as_str().unwrap_or_default());
            lines.push(format!("{fixture}: {surface} {:.2} × {:.2} m = {:.2} m² ({rule})", n(&e["width"]), n(&e["height"]), n(&e["area"])));
            csv.push(format!("\"{fixture}\",{},{surface},\"{on}\",{:.3},{:.3},{:.3},\"{rule}\",{class}", e["fixture"].as_str().unwrap_or_default(), n(&e["width"]), n(&e["height"]), n(&e["area"])));
        }
        lines.push(format!("\nWalls {:.2} m², floors {:.2} m². Copy the schedule as CSV?", n(&s["wall_area"]), n(&s["floor_area"])));
        let d = MessageDialog::builder(&frame, &lines.join("\n"), "Waterproofing").with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconInformation).build();
        if d.show_modal() == ID_YES {
            Clipboard::get().set_text(&csv.join("\n"));
            frame.set_status_text("The waterproofing schedule was copied as CSV", 0);
        }
    }

    /// Architecture › Build › Sauna (ARC-BATH-014): tiers, surfaces and heater, then a click
    /// inside a room closed by walls.
    pub(crate) fn sauna(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Sauna");
        let tiers = f.choice("Benches", &["Two tiers".to_owned(), "One tier".to_owned()], 0);
        let glass = f.text("Uninsulated surface (m²)", "0", "Glass door, tile, brick or glass block: each m² sizes as 1.2 m³ more");
        let logs = f.choice("Walls", &["Insulated, panelled".to_owned(), "Log (× 1.5)".to_owned()], 0);
        let kw = f.text("Heater (kW)", "", "Empty to size it from the room");
        let ok = f.show("Then click inside a room closed by walls, in a plan: benches run along its longer wall, the heater goes on an end wall.", "Choose Room");
        let num = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let mut args = json!({ "command": "bath.sauna", "tiers": if tiers.get_selection() == Some(1) { 1 } else { 2 }, "uninsulated_area": num(&glass).unwrap_or(0.0).max(0.0), "log_walls": logs.get_selection() == Some(1), "replace": true });
        if let Some(k) = num(&kw) {
            args["heater_kw"] = json!(k);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Sauna: click inside the room; Esc ends", 0);
        }
    }

    /// Architecture › Build › Steam Room (ARC-BATH-015): the ceiling, finish and outside wall,
    /// then a click inside a room closed by walls.
    pub(crate) fn steam_room(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Steam Room");
        let height = f.text("Ceiling, high side (mm)", "2130", "Over the bench wall; it falls 1:6 across the room");
        let materials = ["tile", "acrylic", "glass", "stone"];
        let labels: Vec<String> = ["Ceramic tile (× 1.0)", "Acrylic or fibreglass (× 0.65)", "Glass tile or block (× 1.35)", "Natural stone (× 1.65)"].iter().map(|s| (*s).to_owned()).collect();
        let material = f.choice("Walls' finish", &labels, 0);
        let outside = f.choice("Outside wall", &["No".to_owned(), "Yes (+10 %)".to_owned()], 0);
        let ok = f.show("Then click inside a room closed by walls, in a plan: the bench runs along its longer wall, the steam head goes on an end wall, and a floor under it falls to a drain.", "Choose Room");
        let mut args = json!({ "command": "bath.steam_room", "material": materials[material.get_selection().unwrap_or(0) as usize], "exterior_wall": outside.get_selection() == Some(1), "replace": true });
        if let Ok(mm) = height.get_value().trim().parse::<f64>() {
            args["height"] = json!(mm / 1000.0);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(args);
            frame.set_status_text("Steam Room: click inside the room; Esc ends", 0);
        }
    }
}
