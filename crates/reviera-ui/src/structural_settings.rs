// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Structural Settings (STR-SET-001): the project's structural settings in one dialog,
//! by page: symbolic representation, structural steel, the analytical model, boundary conditions
//! and load display. OK applies every change through `structural_settings.set` in one step; an
//! out-of-range value is reported with its rule and nothing changes.

use serde_json::{Map, Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// How a row is edited.
#[derive(Clone, Copy)]
enum Kind {
    /// One of these (label, value).
    Choice(&'static [(&'static str, &'static str)]),
    /// A paper length in millimetres.
    PaperMm,
    /// A model length (display units).
    Length,
    /// A plain number.
    Number,
    Toggle,
    /// The consistency criteria, a check box each.
    Criteria,
}

const SYMBOLS: &[(&str, &str)] = &[("Parallel line", "parallel_line"), ("None", "none")];
const YES_NO: &[(&str, &str)] = &[("No", "false"), ("Yes", "true")];

/// The pages and their rows: (page, label, key, kind, tip).
const ROWS: &[(&str, &str, &str, Kind, &str)] = &[
    (
        "Symbolic Representation",
        "Brace symbol in plan",
        "brace_symbol",
        Kind::Choice(SYMBOLS),
        "How braces are drawn in Coarse plans",
    ),
    (
        "Symbolic Representation",
        "Brace offset (mm)",
        "brace_offset",
        Kind::PaperMm,
        "The parallel line's offset on paper, 0 to 100 mm",
    ),
    (
        "Symbolic Representation",
        "Kicker brace symbol",
        "kicker_symbol",
        Kind::Choice(SYMBOLS),
        "Braces with one end on a beam's span",
    ),
    (
        "Symbolic Representation",
        "Coarse cutback (mm)",
        "coarse_cutback",
        Kind::PaperMm,
        "The gap at joined ends in Coarse views, on paper, 0 to 100 mm",
    ),
    (
        "Structural Steel",
        "Point-to-point steel",
        "point_to_point_steel",
        Kind::Choice(YES_NO),
        "Steel members are not shortened where they join",
    ),
    (
        "Analytical Model",
        "Member supports check",
        "member_supports_check",
        Kind::Toggle,
        "Check that members are supported after each change",
    ),
    (
        "Analytical Model",
        "Analytical/physical consistency check",
        "consistency_check",
        Kind::Toggle,
        "Check the analytical model against the physical one after each change",
    ),
    (
        "Analytical Model",
        "Circular support references",
        "circular_references_check",
        Kind::Toggle,
        "Report members that support each other in a loop",
    ),
    (
        "Analytical Model",
        "Consistency criteria",
        "consistency_criteria",
        Kind::Criteria,
        "C1 disconnected nodes, C2 isolated parts, C3 physical/analytical mismatch, C4 analytical \
         outside physical, C5 no counterpart, C6 overlap not reflected, C7 instability hints, C8 \
         missing properties",
    ),
    (
        "Analytical Model",
        "Support distance",
        "support_distance",
        Kind::Length,
        "Above 0 and at most 2000 mm",
    ),
    (
        "Analytical Model",
        "Analytical-to-physical distance",
        "analytical_distance",
        Kind::Length,
        "Above 0 and at most 2000 mm",
    ),
    (
        "Analytical Model",
        "Node merge distance",
        "node_merge_distance",
        Kind::Length,
        "Smaller than the support distance",
    ),
    (
        "Analytical Model",
        "Level and grid alignment distance",
        "alignment_distance",
        Kind::Length,
        "Above 0 and at most 2000 mm",
    ),
    (
        "Boundary Conditions",
        "Symbol spacing",
        "boundary_spacing",
        Kind::Length,
        "Along line and area conditions, above 0",
    ),
    (
        "Loads Scaling Display",
        "Force (mm per kN)",
        "force_scale",
        Kind::Number,
        "Arrow length on paper per kN",
    ),
    (
        "Loads Scaling Display",
        "Linear force (mm per kN/m)",
        "linear_force_scale",
        Kind::Number,
        "Arrow length on paper per kN/m",
    ),
    (
        "Loads Scaling Display",
        "Area force (mm per kN/m²)",
        "area_force_scale",
        Kind::Number,
        "Arrow length on paper per kN/m²",
    ),
    (
        "Loads Scaling Display",
        "Moment (mm per kN·m)",
        "moment_scale",
        Kind::Number,
        "Arrow length on paper per kN·m",
    ),
    (
        "Loads Scaling Display",
        "Label loads",
        "label_loads",
        Kind::Toggle,
        "Show load values beside the arrows",
    ),
];

enum Control {
    Choice(Choice),
    Text(TextCtrl),
    Checks(Vec<(&'static str, CheckBox)>),
}

const CRITERIA: [&str; 8] = ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8"];

impl Gui {
    pub(crate) fn structural_settings(&mut self) {
        let frame = self.frame;
        let Ok(now) = self.run("structural_settings.get", Value::Null) else {
            return frame.set_status_text("Open a model first", 0);
        };
        let dialog = Dialog::builder(&frame, "Structural Settings")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let mut page = "";
        let mut controls = Vec::new();
        for (p, label, key, kind, tip) in ROWS {
            if *p != page {
                page = p;
                let t = StaticText::builder(&dialog).with_label(p).build();
                let mut font = t.get_font().unwrap_or_default();
                font.set_weight(FontWeight::Bold);
                t.set_font(&font);
                grid.add(&t, 0, SizerFlag::Top, 6);
                let blank = StaticText::builder(&dialog).with_label("").build();
                grid.add(&blank, 0, SizerFlag::Top, 6);
            }
            let l = StaticText::builder(&dialog).with_label(label).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            let value = &now[*key];
            let control = match kind {
                Kind::Choice(items) => {
                    let c = Choice::builder(&dialog)
                        .with_choices(items.iter().map(|i| i.0.to_owned()).collect())
                        .build();
                    let current = match value {
                        Value::Bool(b) => b.to_string(),
                        v => v.as_str().unwrap_or_default().to_owned(),
                    };
                    let at = items.iter().position(|i| i.1 == current).unwrap_or(0);
                    c.set_selection(at as u32);
                    Control::Choice(c)
                }
                Kind::Toggle => {
                    let c = Choice::builder(&dialog)
                        .with_choices(YES_NO.iter().map(|i| i.0.to_owned()).collect())
                        .build();
                    c.set_selection(u32::from(value.as_bool().unwrap_or(false)));
                    Control::Choice(c)
                }
                Kind::Length => {
                    let v = self.format_length(value.as_f64().unwrap_or(0.0));
                    Control::Text(TextCtrl::builder(&dialog).with_value(&v).build())
                }
                Kind::Criteria => {
                    let on: Vec<&str> = value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    Control::Checks(
                        CRITERIA
                            .iter()
                            .map(|c| {
                                let b = CheckBox::builder(&dialog).with_label(c).build();
                                b.set_value(on.contains(c));
                                (*c, b)
                            })
                            .collect(),
                    )
                }
                Kind::PaperMm | Kind::Number => {
                    let v = format!("{}", value.as_f64().unwrap_or(0.0));
                    Control::Text(TextCtrl::builder(&dialog).with_value(&v).build())
                }
            };
            match &control {
                Control::Choice(c) => {
                    c.set_tooltip(tip);
                    grid.add(c, 1, SizerFlag::Expand, 0);
                }
                Control::Text(t) => {
                    t.set_tooltip(tip);
                    t.set_min_size(Size::new(200, -1));
                    grid.add(t, 1, SizerFlag::Expand, 0);
                }
                Control::Checks(boxes) => {
                    let row = BoxSizer::builder(Orientation::Horizontal).build();
                    for (_, b) in boxes {
                        b.set_tooltip(tip);
                        row.add(b, 0, SizerFlag::Right, 6);
                    }
                    grid.add_sizer(&row, 1, SizerFlag::Expand, 0);
                }
            }
            controls.push((*key, *kind, control));
        }
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 12);
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
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let mut values = Map::new();
        for (key, kind, control) in &controls {
            let v = match (kind, control) {
                (Kind::Choice(items), Control::Choice(c)) => {
                    let pick = items[c.get_selection().map_or(0, |i| i as usize)].1;
                    match pick {
                        "true" => json!(true),
                        "false" => json!(false),
                        s => json!(s),
                    }
                }
                (Kind::Toggle, Control::Choice(c)) => json!(c.get_selection() == Some(1)),
                (Kind::Length, Control::Text(t)) => json!(t.get_value()),
                (Kind::Criteria, Control::Checks(boxes)) => json!(
                    boxes
                        .iter()
                        .filter(|(_, b)| b.get_value())
                        .map(|(c, _)| *c)
                        .collect::<Vec<_>>()
                ),
                (_, Control::Text(t)) => match t.get_value().trim().parse::<f64>() {
                    Ok(n) => json!(n),
                    Err(_) => json!(t.get_value()),
                },
                _ => continue,
            };
            values.insert((*key).to_owned(), v);
        }
        dialog.destroy();
        match self.run("structural_settings.set", json!({ "values": values })) {
            Ok(_) => {
                frame.set_status_text("Structural settings changed", 0);
                self.refresh_scene(&frame);
            }
            Err(e) => error(&frame, &format!("The settings were not changed:\n{e}")),
        }
    }
}
