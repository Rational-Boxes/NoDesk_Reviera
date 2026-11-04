// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Manage › Model Properties (UN): the model-wide settings in one place (DM-CODE-001,
//! DM-UNIT-001, ARC-MOD-031). Units (a preset, and the decimal and grouping symbols), the
//! building code with its occupancy and the model's own stair headroom, the accessibility
//! rule set, and the bathroom rule packs (ARC-BATH-001). OK applies what changed through
//! `units.set_preset`, `units.set_number_format`, `code.set`, `clearance.set_rule_set` and
//! `bath.set_rule_packs`. Tools use these as their defaults; a tool may use
//! another code for one operation (the stair opening offer).

use riviera_core::codes::{self, Code, Occupancy};
use riviera_core::units::{Preset, UnitSettings};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PRESETS: [(&str, Preset); 4] = [
    ("Metric (mm)", Preset::MetricMm),
    ("Metric (m)", Preset::MetricM),
    ("Imperial (ft-in)", Preset::ImperialFtIn),
    ("Imperial (decimal ft)", Preset::ImperialDecimalFt),
];
const DECIMALS: [&str; 2] = [".", ","];
const GROUPINGS: [(&str, &str); 4] = [
    ("1,234", ","),
    ("1.234", "."),
    ("1 234", " "),
    ("1'234", "'"),
];
const OCCUPANCIES: [(&str, Occupancy); 2] = [
    ("Within a dwelling unit", Occupancy::Dwelling),
    ("Other (public, commercial, common areas)", Occupancy::Other),
];
const RULES: [(&str, &str); 2] = [
    ("None (door swings only)", "none"),
    ("ADA 2010 (door maneuvering clearances)", "ada_2010"),
];

impl Gui {
    pub(crate) fn model_properties(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open a model first", 0);
        };
        let units = riviera_core::units::settings_of(&store);
        let code = codes::settings(&store);
        let rules = riviera_core::clearances::rule_set(&store);
        let packs = riviera_core::bath_rules::available(&store);
        let packs_chosen = riviera_core::bath_rules::chosen(&store);
        let packs_now: Vec<String> = riviera_core::bath_rules::active(&store).into_iter().map(|p| p.id).collect();
        let preset_now = PRESETS.iter().position(|(_, p)| {
            UnitSettings::preset(*p).formats.get("length") == units.formats.get("length")
        });

        let dialog = Dialog::builder(&frame, "Model Properties")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let heading = |text: &str| {
            let t = StaticText::builder(&dialog).with_label(text).build();
            let mut font = t.get_font().unwrap_or_default();
            font.set_weight(FontWeight::Bold);
            t.set_font(&font);
            grid.add(&t, 0, SizerFlag::Top, 6);
            // An empty cell (a spacer does not take one).
            let blank = StaticText::builder(&dialog).with_label("").build();
            grid.add(&blank, 0, SizerFlag::Top, 6);
        };
        let choice = |label: &str, items: Vec<String>, at: usize, tip: &str| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let c = Choice::builder(&dialog).with_choices(items).build();
            c.set_selection(at as u32);
            c.set_tooltip(tip);
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&c, 1, SizerFlag::Expand, 0);
            c
        };

        heading("Units");
        let mut preset_items: Vec<String> = PRESETS.iter().map(|p| p.0.to_owned()).collect();
        if preset_now.is_none() {
            preset_items.insert(0, "Custom (as set)".to_owned());
        }
        let preset = choice(
            "Units",
            preset_items,
            preset_now.unwrap_or(0),
            "Lengths, areas, volumes and angles as the preset formats them",
        );
        let decimal = choice(
            "Decimal symbol",
            DECIMALS.iter().map(|d| (*d).to_owned()).collect(),
            DECIMALS
                .iter()
                .position(|d| *d == units.decimal_symbol)
                .unwrap_or(0),
            "The symbol before the decimals",
        );
        let grouping = choice(
            "Digit grouping",
            GROUPINGS.iter().map(|g| g.0.to_owned()).collect(),
            GROUPINGS
                .iter()
                .position(|g| g.1 == units.grouping_symbol)
                .unwrap_or(0),
            "The symbol between groups of digits",
        );

        heading("Building code");
        let code_choice = choice(
            "Code",
            Code::ALL.iter().map(|c| c.label().to_owned()).collect(),
            Code::ALL.iter().position(|c| *c == code.code).unwrap_or(0),
            "The code tools follow by default (a tool may use another for one operation)",
        );
        let occupancy = choice(
            "Occupancy",
            OCCUPANCIES.iter().map(|o| o.0.to_owned()).collect(),
            OCCUPANCIES
                .iter()
                .position(|o| o.1 == code.occupancy)
                .unwrap_or(0),
            "Where the code asks differently of dwellings and other buildings",
        );
        let l = StaticText::builder(&dialog)
            .with_label("Stair headroom")
            .build();
        let own = code
            .headroom
            .map(|h| self.format_length(h))
            .unwrap_or_default();
        let headroom = TextCtrl::builder(&dialog).with_value(&own).build();
        headroom.set_min_size(Size::new(220, -1));
        headroom.set_tooltip("The model's own headroom over stairs; empty uses the code's");
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&headroom, 1, SizerFlag::Expand, 0);
        // What the code asks, for reference.
        let reference = code.code.headroom(code.occupancy).map_or_else(
            || "Custom: set the headroom above".to_owned(),
            |h| format!("Code: {} — {}", self.format_length(h.stair), h.clause),
        );
        let blank = StaticText::builder(&dialog).with_label("").build();
        grid.add(&blank, 0, SizerFlag::AlignCenterVertical, 0);
        let note = StaticText::builder(&dialog).with_label(&reference).build();
        note.wrap(380);
        grid.add(&note, 1, SizerFlag::Expand, 0);

        heading("Accessibility");
        let rule = choice(
            "Rule set",
            RULES.iter().map(|r| r.0.to_owned()).collect(),
            match rules {
                riviera_core::clearances::RuleSet::None => 0,
                riviera_core::clearances::RuleSet::Ada2010 => 1,
            },
            "Clearances checked as virtual volumes (Check Overlaps)",
        );

        heading("Bathroom rules");
        let blank = StaticText::builder(&dialog).with_label("").build();
        grid.add(&blank, 0, SizerFlag::AlignCenterVertical, 0);
        let auto = CheckBox::builder(&dialog)
            .with_label("Choose from the building code and accessibility rule set")
            .build();
        auto.set_value(packs_chosen.is_none());
        auto.set_tooltip("The US plumbing codes for the IBC or IRC, Approved Document M for England, and the ADA with the ADA rule set");
        grid.add(&auto, 1, SizerFlag::Expand, 0);
        let pack_boxes: Vec<(String, CheckBox)> = packs
            .iter()
            .map(|p| {
                let layer = match p.layer {
                    riviera_core::bath_rules::Layer::Minimum => "Code",
                    riviera_core::bath_rules::Layer::Accessible => "Accessibility",
                    riviera_core::bath_rules::Layer::Recommended => "Guidance",
                };
                let l = StaticText::builder(&dialog).with_label(layer).build();
                let b = CheckBox::builder(&dialog).with_label(&p.title).build();
                b.set_value(packs_now.contains(&p.id));
                b.set_tooltip(&p.edition);
                grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&b, 1, SizerFlag::Expand, 0);
                (p.id.clone(), b)
            })
            .collect();
        let blank = StaticText::builder(&dialog).with_label("").build();
        grid.add(&blank, 0, SizerFlag::AlignCenterVertical, 0);
        let note = StaticText::builder(&dialog)
            .with_label("Ticking packs yourself unticks the automatic choice. Fixtures' clear spaces show as virtual volumes; Check Bathrooms reports what is in them.")
            .build();
        note.wrap(380);
        grid.add(&note, 1, SizerFlag::Expand, 0);

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
        let sel = |c: &Choice| c.get_selection().map_or(0, |i| i as usize);
        let preset_at = sel(&preset);
        let preset_pick = if preset_now.is_none() {
            preset_at.checked_sub(1).map(|i| PRESETS[i].1)
        } else {
            Some(PRESETS[preset_at].1)
        };
        let decimal_pick = DECIMALS[sel(&decimal)];
        let grouping_pick = GROUPINGS[sel(&grouping)].1;
        let code_pick = Code::ALL[sel(&code_choice)];
        let occupancy_pick = OCCUPANCIES[sel(&occupancy)].1;
        let headroom_text = headroom.get_value();
        let rule_pick = RULES[sel(&rule)].1;
        let auto_pick = auto.is_checked();
        let packs_pick: Vec<String> = pack_boxes.iter().filter(|(_, b)| b.is_checked()).map(|(id, _)| id.clone()).collect();
        dialog.destroy();

        let mut problems = Vec::new();
        if let Some(p) = preset_pick
            && Some(PRESETS.iter().position(|x| x.1 == p).unwrap_or(0)) != preset_now
            && let Err(e) = self.run("units.set_preset", json!({ "preset": p }))
        {
            problems.push(e);
        }
        if (decimal_pick != units.decimal_symbol || grouping_pick != units.grouping_symbol)
            && let Err(e) = self.run(
                "units.set_number_format",
                json!({ "decimal_symbol": decimal_pick, "digit_grouping": grouping_pick }),
            )
        {
            problems.push(e);
        }
        let headroom_value = if headroom_text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            json!(headroom_text)
        };
        let headroom_changed = headroom_text.trim() != own.trim();
        if code_pick != code.code || occupancy_pick != code.occupancy || headroom_changed {
            let mut args = json!({ "code": code_pick.key(), "occupancy": occupancy_pick });
            if headroom_changed {
                args["headroom"] = headroom_value;
            }
            if let Err(e) = self.run("code.set", args) {
                problems.push(e);
            }
        }
        let rule_now = RULES[match rules {
            riviera_core::clearances::RuleSet::None => 0,
            riviera_core::clearances::RuleSet::Ada2010 => 1,
        }]
        .1;
        if rule_pick != rule_now
            && let Err(e) = self.run("clearance.set_rule_set", json!({ "standard": rule_pick }))
        {
            problems.push(e);
        }
        // Packs ticked by hand, unless the ticks are just the automatic choice left alone.
        let automatic = auto_pick && packs_pick == packs_now;
        let packs_args = if automatic { json!({}) } else { json!({ "packs": packs_pick }) };
        let packs_change = if automatic { packs_chosen.is_some() } else { packs_chosen.as_ref() != Some(&packs_pick) };
        if packs_change && let Err(e) = self.run("bath.set_rule_packs", packs_args) {
            problems.push(e);
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        if problems.is_empty() {
            frame.set_status_text("Model properties set", 0);
        } else {
            error(
                &frame,
                &format!("Some settings were not changed:\n{}", problems.join("\n")),
            );
        }
    }
}
