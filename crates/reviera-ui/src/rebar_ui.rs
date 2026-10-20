// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Concrete › Reinforcement (STR-RBR-001…004, 008, 009). *Rebar* places a set in each selected
//! concrete host by Expand to Host: the dialog offers the placements the host takes (links in a
//! column's or beam's section, bars at one of its faces, a floor's top or bottom layer, a wall's
//! faces), the shapes and bar types, and the layout rule. *Cover* gives the selected hosts' faces
//! a cover type; *Reinforcement Settings* holds rounding, hooks in shape definitions and the
//! cover types. A selected set's Properties show its bars and layout, with the layout editable.

use reviera_core::ElementId;
use reviera_core::rebar::HostKind;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// The placements a host takes: (label, placement, shapes).
fn placements(kind: HostKind) -> Vec<(&'static str, Value, &'static [&'static str])> {
    match kind {
        HostKind::Column | HostKind::Beam => vec![
            ("Links in the section", json!({ "kind": "section" }), &["51"]),
            ("Bars at the top face", json!({ "kind": "face", "face": "top" }), &["00", "11", "21"]),
            ("Bars at the bottom face", json!({ "kind": "face", "face": "bottom" }), &["00", "11", "21"]),
            ("Bars at the left face", json!({ "kind": "face", "face": "left" }), &["00", "11", "21"]),
            ("Bars at the right face", json!({ "kind": "face", "face": "right" }), &["00", "11", "21"]),
        ],
        HostKind::Floor => vec![
            ("Bottom layer", json!({ "kind": "layer", "face": "bottom" }), &["00"]),
            ("Top layer", json!({ "kind": "layer", "face": "top" }), &["00"]),
        ],
        HostKind::Wall => vec![
            ("Exterior face, vertical", json!({ "kind": "layer", "face": "exterior", "vertical": true }), &["00"]),
            ("Exterior face, horizontal", json!({ "kind": "layer", "face": "exterior", "vertical": false }), &["00"]),
            ("Interior face, vertical", json!({ "kind": "layer", "face": "interior", "vertical": true }), &["00"]),
            ("Interior face, horizontal", json!({ "kind": "layer", "face": "interior", "vertical": false }), &["00"]),
        ],
    }
}

const RULES: &[(&str, &str)] = &[
    ("Maximum spacing", "maximum_spacing"),
    ("Fixed number", "fixed_number"),
    ("Number with spacing", "number_with_spacing"),
    ("Minimum clear spacing", "minimum_clear_spacing"),
    ("Single", "single"),
    ("Custom formula", "custom"),
];

/// A layout from the dialog's fields (mm in, metres out).
fn layout(rule: &str, n: &str, spacing: &str, formula: &str) -> Value {
    let mm = |t: &str| t.trim().parse::<f64>().ok().map(|v| v / 1000.0);
    let mut l = json!({ "rule": rule });
    match rule {
        "fixed_number" => l["n"] = json!(n.trim().parse::<usize>().unwrap_or(0)),
        "number_with_spacing" => {
            l["n"] = json!(n.trim().parse::<usize>().unwrap_or(0));
            l["spacing"] = json!(mm(spacing));
        }
        "maximum_spacing" => l["spacing"] = json!(mm(spacing)),
        "minimum_clear_spacing" => l["clear"] = json!(mm(spacing)),
        "custom" => l["formula"] = json!(formula.trim()),
        _ => {}
    }
    l
}

pub(super) fn row(dialog: &Dialog, grid: &FlexGridSizer, label: &str) {
    let l = StaticText::builder(dialog).with_label(label).build();
    grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
}

impl Gui {
    fn rebar_hosts(&mut self) -> Vec<(ElementId, HostKind)> {
        let Some(store) = self.store_now() else { return Vec::new() };
        self.selection()
            .into_iter()
            .filter_map(|id| store.get(id).and_then(|e| reviera_core::rebar::host_check(&store, e).ok()).map(|k| (id, k)))
            .collect()
    }

    /// Concrete › Reinforcement › Rebar.
    pub(crate) fn place_rebar(&mut self) {
        let frame = self.frame;
        let hosts = self.rebar_hosts();
        let Some(&(_, kind)) = hosts.first() else {
            let why = self
                .selection()
                .first()
                .and_then(|id| self.run("rebar.can_host", json!({ "element_id": id })).ok())
                .and_then(|c| c["reason"].as_str().map(str::to_owned));
            return frame.set_status_text(
                &match why {
                    Some(w) => format!("Rebar: the selection cannot host rebar ({w})"),
                    None => "Rebar: select concrete columns, beams, structural floors or walls first".to_owned(),
                },
                0,
            );
        };
        let hosts: Vec<ElementId> = hosts.iter().filter(|h| h.1 == kind).map(|h| h.0).collect();
        let options = placements(kind);
        let bars = self.run("rebar_bar_type.list", json!({})).unwrap_or_default();
        let bar_ids: Vec<String> = bars.as_array().into_iter().flatten().filter_map(|b| b["id"].as_str().map(str::to_owned)).collect();
        let bar_names: Vec<String> = bars.as_array().into_iter().flatten().filter_map(|b| b["name"].as_str().map(str::to_owned)).collect();
        let shapes = self.run("rebar_shape.list", json!({})).unwrap_or_default();
        let shape_name = |code: &str| {
            shapes["shapes"]
                .as_array()
                .and_then(|s| s.iter().find(|s| s["code"] == code))
                .and_then(|s| s["name"].as_str())
                .map_or_else(|| code.to_owned(), |n| format!("{code} {n}"))
        };
        let all_shapes: Vec<&str> = {
            let mut v: Vec<&str> = options.iter().flat_map(|o| o.2.iter().copied()).collect();
            v.dedup();
            v.sort_unstable();
            v.dedup();
            v
        };

        let dialog = Dialog::builder(&frame, "Rebar").with_style(DialogStyle::DefaultDialogStyle).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        grid.add_growable_col(1, 1);
        row(&dialog, &grid, "Placement");
        let place = Choice::builder(&dialog).with_choices(options.iter().map(|o| o.0.to_owned()).collect()).build();
        place.set_selection(0);
        grid.add(&place, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Shape");
        let shape = Choice::builder(&dialog).with_choices(all_shapes.iter().map(|c| shape_name(c)).collect()).build();
        // The first placement's own shape (links take 51); a shape the placement does not take
        // is replaced by its first.
        shape.set_selection(all_shapes.iter().position(|c| *c == options[0].2[0]).unwrap_or(0) as u32);
        shape.set_tooltip("Links take 51; bars at a face 00, 11 or 21; floor and wall layers 00");
        grid.add(&shape, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Bar type");
        let bar = Choice::builder(&dialog).with_choices(bar_names.clone()).build();
        let default_bar = if kind == HostKind::Column || kind == HostKind::Beam { 2 } else { 3 };
        bar.set_selection(default_bar.min(bar_names.len().saturating_sub(1)) as u32);
        grid.add(&bar, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Bar direction in plan (°)");
        let angle = TextCtrl::builder(&dialog).with_value("0").build();
        angle.set_tooltip("Floors: the bars' direction, 0 along x");
        grid.add(&angle, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Layout");
        let rule = Choice::builder(&dialog).with_choices(RULES.iter().map(|r| r.0.to_owned()).collect()).build();
        rule.set_selection(0);
        grid.add(&rule, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Number of bars");
        let n = TextCtrl::builder(&dialog).with_value("4").build();
        grid.add(&n, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Spacing or clear spacing (mm)");
        let spacing = TextCtrl::builder(&dialog).with_value("200").build();
        grid.add(&spacing, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Formula (mm)");
        let formula = TextCtrl::builder(&dialog).with_value("3x100; ?x200; 3x100").build();
        formula.set_tooltip("count x spacing or distance @ spacing, separated by ;  one ? count takes the rest");
        grid.add(&formula, 1, SizerFlag::Expand, 0);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 12);
        let note = StaticText::builder(&dialog)
            .with_label(&format!("{} host(s); bars keep inside each face's cover", hosts.len()))
            .build();
        sizer.add(&note, 0, SizerFlag::Left | SizerFlag::Right, 12);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Place").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        ok.set_default();
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let at = |c: &Choice| c.get_selection().map_or(0, |i| i as usize);
        let (label, mut placement, allowed) = options[at(&place).min(options.len() - 1)].clone();
        let code = all_shapes[at(&shape).min(all_shapes.len() - 1)];
        let code = if allowed.contains(&code) { code } else { allowed[0] };
        if kind == HostKind::Floor {
            placement["angle"] = json!(angle.get_value().trim().parse::<f64>().unwrap_or(0.0));
        }
        let bar_id = bar_ids.get(at(&bar)).cloned().unwrap_or_default();
        let lay = layout(RULES[at(&rule).min(RULES.len() - 1)].1, &n.get_value(), &spacing.get_value(), &formula.get_value());
        dialog.destroy();
        let (mut placed, mut failed) = (Vec::new(), Vec::new());
        for h in &hosts {
            match self.run(
                "rebar.place",
                json!({ "host_id": h, "shape": code, "bar_type": bar_id, "placement": placement, "layout": lay }),
            ) {
                Ok(v) => placed.push(v),
                Err(e) => failed.push(format!("{h}: {e}")),
            }
        }
        self.refresh_scene(&frame);
        if !failed.is_empty() {
            error(&frame, &format!("Rebar was not placed in every host:\n{}", failed.join("\n")));
        }
        frame.set_status_text(&format!("{label}: {} set(s) of shape {code} placed", placed.len()), 0);
    }

    /// Concrete › Reinforcement › Cover: a cover type for the selected hosts' faces.
    pub(crate) fn rebar_cover(&mut self) {
        let frame = self.frame;
        let hosts = self.rebar_hosts();
        if hosts.is_empty() {
            return frame.set_status_text("Cover: select concrete hosts first", 0);
        }
        let Ok(s) = self.run("reinforcement_settings.get", json!({})) else { return };
        let covers: Vec<(String, String)> = s["cover_types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                (
                    c["id"].as_str().unwrap_or("").to_owned(),
                    format!("{} ({:.0} mm)", c["name"].as_str().unwrap_or(""), c["offset"].as_f64().unwrap_or(0.0) * 1000.0),
                )
            })
            .collect();
        let faces = ["All faces", "top", "bottom", "exterior", "interior", "other"];
        let d = SingleChoiceDialog::builder(&frame, "Faces:", "Rebar Cover", &faces).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let face = usize::try_from(d.get_selection()).ok().filter(|i| *i > 0).map(|i| faces[i]);
        let labels: Vec<&str> = covers.iter().map(|c| c.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Cover type:", "Rebar Cover", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(cover) = usize::try_from(d.get_selection()).ok().and_then(|i| covers.get(i)) else { return };
        let ids: Vec<ElementId> = hosts.iter().map(|h| h.0).collect();
        // Hosts without the face (a column has no exterior) are given their other faces.
        let mut args = json!({ "host_ids": ids, "cover_type_id": cover.0 });
        if let Some(f) = face {
            args["face"] = json!(f);
        }
        match self.run("rebar_cover.assign", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                frame.set_status_text(&format!("Cover {} given to {} host(s)", cover.1, hosts.len()), 0);
            }
            Err(e) => error(&frame, &format!("The cover was not changed:\n{e}")),
        }
    }

    /// Concrete › Settings › Reinforcement Settings: rounding, hooks in shape definitions and cover types.
    pub(crate) fn reinforcement_settings(&mut self) {
        let frame = self.frame;
        let Ok(s) = self.run("reinforcement_settings.get", json!({})) else {
            return frame.set_status_text("Open a model first", 0);
        };
        let dialog = Dialog::builder(&frame, "Reinforcement Settings").with_style(DialogStyle::DefaultDialogStyle).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        grid.add_growable_col(1, 1);
        let r = &s["rounding"];
        row(&dialog, &grid, "Rounding");
        let on = CheckBox::builder(&dialog).with_label("Round bar lengths").build();
        on.set_value(r["on"] == true);
        grid.add(&on, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Method");
        let methods = ["nearest", "up", "down"];
        let method = Choice::builder(&dialog).with_choices(vec!["Nearest".into(), "Up".into(), "Down".into()]).build();
        method.set_selection(methods.iter().position(|m| r["method"] == *m).unwrap_or(0) as u32);
        grid.add(&method, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Bar length increment (mm)");
        let bar_len = TextCtrl::builder(&dialog).with_value(&format!("{}", r["bar_length"].as_f64().unwrap_or(0.001) * 1000.0)).build();
        grid.add(&bar_len, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Segment length increment (mm)");
        let seg_len = TextCtrl::builder(&dialog).with_value(&format!("{}", r["segment_length"].as_f64().unwrap_or(0.005) * 1000.0)).build();
        grid.add(&seg_len, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Shapes");
        let hooks = CheckBox::builder(&dialog).with_label("Include hooks in shape definitions").build();
        hooks.set_value(s["hooks_in_shape"] != false);
        let locked = s["locked"]["hooks_in_shape"].as_str().map(str::to_owned);
        if let Some(why) = &locked {
            hooks.enable(false);
            hooks.set_tooltip(&format!("Locked: {why}"));
        }
        grid.add(&hooks, 1, SizerFlag::Expand, 0);
        row(&dialog, &grid, "Presentation of new sets");
        let modes = ["all", "first_last", "middle"];
        let presentation = Choice::builder(&dialog).with_choices(vec!["Show All".into(), "First/Last".into(), "Middle".into()]).build();
        presentation.set_selection(modes.iter().position(|m| s["presentation"] == *m).unwrap_or(0) as u32);
        presentation.set_tooltip("How sets show in views that give them no presentation of their own");
        grid.add(&presentation, 1, SizerFlag::Expand, 0);
        // Cover types: one per line, `name = offset mm`; new lines make new types.
        row(&dialog, &grid, "Cover types (name = mm)");
        let lines: Vec<String> = s["cover_types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let id = c["id"].as_str().unwrap_or("");
                format!(
                    "{} = {} ({} use(s))",
                    c["name"].as_str().unwrap_or(""),
                    c["offset"].as_f64().unwrap_or(0.0) * 1000.0,
                    s["cover_usage"][id].as_u64().unwrap_or(0)
                )
            })
            .collect();
        let covers = TextCtrl::builder(&dialog)
            .with_value(&lines.join("\n"))
            .with_style(TextCtrlStyle::MultiLine)
            .build();
        covers.set_min_size(Size::new(320, 110));
        covers.set_tooltip("Change an offset, or add a line `Name = mm` for a new cover type");
        grid.add(&covers, 1, SizerFlag::Expand, 0);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 12);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        ok.set_default();
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let mm = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok().map(|v| v / 1000.0);
        let mut steps: Vec<(&str, Value)> = vec![
            ("reinforcement_settings.set", json!({ "key": "rounding.on", "value": on.get_value() })),
            (
                "reinforcement_settings.set",
                json!({ "key": "rounding.method", "value": methods[method.get_selection().map_or(0, |i| i as usize).min(2)] }),
            ),
        ];
        if let Some(v) = mm(&bar_len) {
            steps.push(("reinforcement_settings.set", json!({ "key": "rounding.bar_length", "value": v })));
        }
        if let Some(v) = mm(&seg_len) {
            steps.push(("reinforcement_settings.set", json!({ "key": "rounding.segment_length", "value": v })));
        }
        if locked.is_none() {
            steps.push(("reinforcement_settings.set", json!({ "key": "hooks_in_shape", "value": hooks.get_value() })));
        }
        steps.push((
            "reinforcement_settings.set",
            json!({ "key": "presentation", "value": modes[presentation.get_selection().map_or(0, |i| i as usize).min(2)] }),
        ));
        let existing: Vec<(String, String, f64)> = s["cover_types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (c["id"].as_str().unwrap_or("").to_owned(), c["name"].as_str().unwrap_or("").to_owned(), c["offset"].as_f64().unwrap_or(0.0)))
            .collect();
        for line in covers.get_value().lines() {
            let Some((name, rest)) = line.split_once('=') else { continue };
            let name = name.trim();
            let Some(v) = rest.split_whitespace().next().and_then(|t| t.parse::<f64>().ok()).map(|v| v / 1000.0) else { continue };
            match existing.iter().find(|c| c.1 == name) {
                Some(c) if (c.2 - v).abs() > 1e-9 => {
                    steps.push(("rebar_cover_type.set", json!({ "cover_type_id": c.0, "offset": v })));
                }
                Some(_) => {}
                None if !name.is_empty() => steps.push(("rebar_cover_type.create", json!({ "name": name, "offset": v }))),
                None => {}
            }
        }
        dialog.destroy();
        let mut errors = Vec::new();
        for (cmd, args) in steps {
            if let Err(e) = self.run(cmd, args) {
                errors.push(e.to_string());
            }
        }
        self.refresh_scene(&frame);
        self.properties_stale = true;
        if errors.is_empty() {
            frame.set_status_text("Reinforcement settings changed", 0);
        } else {
            error(&frame, &format!("Some settings were not changed:\n{}", errors.join("\n")));
        }
    }

    /// The rows of a rebar set; `false` if `id` is not one.
    pub(crate) fn rebar_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        if self.store_now().and_then(|s| s.get(id).and_then(reviera_core::rebar::rebar_recipe)).is_none() {
            return false;
        }
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [id] })) else {
            grid.append(Property::category("Rebar", "c_rebar"));
            grid.append(Property::string("Status", "rb_status", "Its bars cannot be worked out (the host changed?)"));
            grid.set_property_read_only("rb_status", true);
            return true;
        };
        let i = &i[0];
        grid.append(Property::category("Rebar", "c_rebar"));
        let ro = |grid: &PropertyGrid, label: &str, name: &str, value: String| {
            grid.append(Property::string(label, name, value));
            grid.set_property_read_only(name, true);
        };
        ro(grid, "Rebar Number", "rb_number", i["number"].as_u64().map_or(String::new(), |n| n.to_string()));
        grid.append(Property::string("Partition", "rb_partition", i["partition"].as_str().unwrap_or("").to_owned()));
        grid.set_help_string("rb_partition", "Identical sets share a number within a partition (phase, location, top or bottom, …)");
        ro(grid, "Shape", "rb_shape", i["shape"].as_str().unwrap_or("").to_owned());
        ro(grid, "Bar type", "rb_bar", i["bar_type"].as_str().unwrap_or("").to_owned());
        ro(grid, "Quantity", "rb_count", format!("{}", i["count"].as_u64().unwrap_or(0)));
        ro(grid, "Bar length (mm)", "rb_length", i["length_text"].as_str().unwrap_or("").to_owned());
        ro(grid, "Total bar length", "rb_total", self.format_length(i["total_length"].as_f64().unwrap_or(0.0)));
        ro(grid, "Mass (kg)", "rb_mass", format!("{:.1}", i["mass"].as_f64().unwrap_or(0.0)));
        let dims: Vec<String> = i["dimensions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| format!("{} {:.0}", d[0].as_str().unwrap_or(""), d[1].as_f64().unwrap_or(0.0) * 1000.0))
            .collect();
        ro(grid, "Dimensions (mm)", "rb_dims", dims.join(", "));
        // Each segment can be set by hand (STR-RBR-006); blank resets it.
        for d in i["dimensions"].as_array().into_iter().flatten() {
            let letter = d[0].as_str().unwrap_or("");
            let name = format!("rb_seg_{letter}");
            grid.append(Property::string(format!("Segment {letter} (mm)"), &name, format!("{:.0}", d[1].as_f64().unwrap_or(0.0) * 1000.0)));
            grid.set_help_string(&name, "Set this segment's length; blank goes back to what the host gives");
        }
        if i["orphaned"] == true {
            ro(grid, "Orphaned", "rb_orphan", i["orphan_reason"].as_str().unwrap_or("").to_owned());
        }
        grid.append(Property::category("Layout", "c_rebar_layout"));
        let l = &i["layout"];
        let current = RULES.iter().position(|r| l["rule"] == r.1).unwrap_or(0) as i32;
        grid.append(Property::enumeration(
            "Rule",
            "rb_rule",
            RULES.iter().enumerate().map(|(k, r)| (r.0.to_owned(), k as i32)).collect::<Vec<(String, i32)>>(),
            current,
        ));
        let mm = |v: &Value| v.as_f64().map_or(String::new(), |x| format!("{}", (x * 1000.0 * 1000.0).round() / 1000.0));
        grid.append(Property::string("Number", "rb_n", l["n"].as_u64().map_or(String::new(), |n| n.to_string())));
        grid.append(Property::string("Spacing (mm)", "rb_spacing", mm(if l["rule"] == "minimum_clear_spacing" { &l["clear"] } else { &l["spacing"] })));
        grid.append(Property::string("Formula (mm)", "rb_formula", l["formula"].as_str().unwrap_or("").to_owned()));
        grid.set_help_string("rb_formula", "count x spacing or distance @ spacing, separated by ;  one ? count takes the rest");
        ro(grid, "Array length", "rb_array", self.format_length(i["array_length"].as_f64().unwrap_or(0.0)));
        true
    }

    /// An edit of a rebar layout row; `false` if `property` is not one.
    pub(crate) fn rebar_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("rb_") {
            return false;
        }
        let frame = self.frame;
        let Some(id) = self.selection().first().copied() else { return true };
        if let Some(letter) = property.strip_prefix("rb_seg_") {
            let length = text.as_deref().and_then(|t| t.trim().parse::<f64>().ok()).map(|v| v / 1000.0);
            if let Err(e) = self.run("rebar.set_segment_length", json!({ "rebar_id": id, "segment": letter, "length": length })) {
                error(&frame, &format!("The segment was not changed:\n{e}"));
            }
            self.refresh_scene(&frame);
            self.properties_stale = true;
            return true;
        }
        if property == "rb_partition" {
            let ids: Vec<ElementId> = self.selection();
            if let Err(e) = self.run("rebar.set_partition", json!({ "ids": ids, "name": text.unwrap_or_default() })) {
                error(&frame, &format!("The partition was not changed:\n{e}"));
            }
            self.properties_stale = true;
            return true;
        }
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [id] })) else { return true };
        let l = &i[0]["layout"];
        let text = text.unwrap_or_default();
        let mut rule = l["rule"].as_str().unwrap_or("maximum_spacing").to_owned();
        let mut n = l["n"].as_u64().map_or_else(|| format!("{}", i[0]["count"]), |n| n.to_string());
        let spacing_now = if rule == "minimum_clear_spacing" { &l["clear"] } else { &l["spacing"] };
        let mut spacing = spacing_now.as_f64().map_or("200".to_owned(), |x| format!("{}", x * 1000.0));
        let mut formula = l["formula"].as_str().unwrap_or("").to_owned();
        match property {
            "rb_rule" => {
                if let Some(r) = RULES.iter().find(|r| r.0 == text) {
                    rule = r.1.to_owned();
                }
            }
            "rb_n" => n = text,
            "rb_spacing" => spacing = text,
            "rb_formula" => formula = text,
            _ => return true,
        }
        if rule == "custom" && formula.trim().is_empty() {
            formula = "?x200".into();
        }
        let mut args = layout(&rule, &n, &spacing, &formula);
        args["rebar_ids"] = json!([id]);
        if let Err(e) = self.run("rebar.set_layout", args) {
            error(&frame, &format!("The layout was not changed:\n{e}"));
        }
        self.refresh_scene(&frame);
        self.properties_stale = true;
        true
    }
}
