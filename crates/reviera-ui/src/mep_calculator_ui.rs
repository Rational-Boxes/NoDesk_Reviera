// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Sizing Calculator palette (MEP-CALC-005): a docked pane sizing one duct or pipe by hand.
//! With one segment selected it reads its flow, shape and roughness and **Apply** resizes it
//! (`duct.set_size` / `pipe.set_size`, refused on size-locked segments); otherwise it is a
//! calculator for a typed flow. Friction and velocity targets each with a ± tolerance slider, an
//! aspect-ratio maximum, roughness and restrictions; **Show Best Sizes** fills a table sortable by
//! any column, the row closest to the friction target selected; **Swap Height/Width** turns the
//! rectangles. Its inputs are session state, not stored in the model.

use std::cell::RefCell;
use std::rc::Rc;

use reviera_core::ElementId;
use reviera_core::mep::{Domain, Shape};
use reviera_core::mep_calculator::{Candidate, CandidatesResult};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error, with};

pub(crate) struct SizingCalculator {
    element_label: StaticText,
    flow: TextCtrl,
    user_flow: CheckBox,
    domain: Choice,
    shape: Choice,
    friction_on: CheckBox,
    friction: TextCtrl,
    friction_tol: Slider,
    velocity_on: CheckBox,
    velocity: TextCtrl,
    velocity_tol: Slider,
    aspect_on: CheckBox,
    aspect: TextCtrl,
    roughness: TextCtrl,
    height: TextCtrl,
    width: TextCtrl,
    diameter: TextCtrl,
    list: ListCtrl,
    note: StaticText,
    apply: Button,
    rows: Vec<Candidate>,
    swap: bool,
    /// The segment the inputs were read from.
    element: Option<(ElementId, Domain)>,
    /// The selection they were read for.
    seen: Option<Vec<ElementId>>,
}

const COLUMNS: [(&str, i32); 7] =
    [("Size", 90), ("Friction (Pa/m)", 95), ("Velocity (m/s)", 90), ("Aspect", 55), ("Eq. Ø (mm)", 75), ("Friction Δ %", 80), ("Velocity Δ %", 80)];

fn num(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

fn cell(c: &Candidate, k: usize) -> String {
    let pct = |d: Option<f64>| d.map(|d| format!("{:+.1}", d * 100.0)).unwrap_or_default();
    match k {
        0 => if c.within { c.size.clone() } else { format!("{} (outside)", c.size) },
        1 => format!("{:.3}", c.friction),
        2 => format!("{:.2}", c.velocity),
        3 => c.aspect.map(|a| format!("1:{a:.2}")).unwrap_or_default(),
        4 => format!("{:.0}", c.equivalent_diameter * 1000.0),
        5 => pct(c.friction_deviation),
        _ => pct(c.velocity_deviation),
    }
}

/// A sort key of a column.
fn key(c: &Candidate, k: usize) -> f64 {
    match k {
        0 => c.equivalent_diameter,
        1 => c.friction,
        2 => c.velocity,
        3 => c.aspect.unwrap_or(1.0),
        4 => c.equivalent_diameter,
        5 => c.friction_deviation.map_or(f64::INFINITY, f64::abs),
        _ => c.velocity_deviation.map_or(f64::INFINITY, f64::abs),
    }
}

impl Gui {
    /// Shows or hides the Sizing Calculator, making it on first use.
    pub(crate) fn toggle_sizing_calculator(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().calculator.is_some() {
            with(gui, |g| {
                g.toggle_pane("sizing_calculator");
                g.calculator_sync(true);
            });
            return;
        }
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(4).with_hgap(8).build();
        grid.add_growable_col(1, 1);
        let label = |t: &str| {
            let s = StaticText::builder(&panel).with_label(t).build();
            grid.add(&s, 0, SizerFlag::AlignCenterVertical, 0);
        };
        let text = |v: &str, tip: &str| {
            let t = TextCtrl::builder(&panel).with_value(v).build();
            t.set_tooltip(tip);
            t
        };
        fn pair<A: WxWidget, B: WxWidget>(grid: &FlexGridSizer, a: &A, b: &B) {
            let s = BoxSizer::builder(Orientation::Horizontal).build();
            s.add(a, 1, SizerFlag::Expand | SizerFlag::Right, 4);
            s.add(b, 1, SizerFlag::Expand, 0);
            grid.add_sizer(&s, 1, SizerFlag::Expand, 0);
        }
        let element_label = StaticText::builder(&panel).with_label("Nothing selected: calculator mode").build();
        label("Flow (l/s)");
        let flow = text("1000", "The flow to size for");
        let user_flow = CheckBox::builder(&panel).with_label("User value").build();
        user_flow.set_tooltip("Use the flow typed here, not the selected element's");
        pair(&grid, &flow, &user_flow);
        label("Duct or pipe");
        let domain = Choice::builder(&panel).with_choices(vec!["Duct".into(), "Pipe".into()]).build();
        domain.set_selection(0);
        let shape = Choice::builder(&panel).with_choices(vec!["Round".into(), "Rectangular".into(), "Oval".into()]).build();
        shape.set_selection(1);
        pair(&grid, &domain, &shape);
        let band = |name: &str, value: &str, unit: &str, on: bool| {
            let c = CheckBox::builder(&panel).with_label(name).build();
            c.set_value(on);
            grid.add(&c, 0, SizerFlag::AlignCenterVertical, 0);
            let t = text(value, &format!("Target ({unit})"));
            let s = Slider::builder(&panel).with_value(10).with_min_value(0).with_max_value(50).build();
            s.set_tooltip("Tolerance: ± 0–50 % (10 % by default)");
            // The slider with its value beside it.
            let tol = BoxSizer::builder(Orientation::Horizontal).build();
            let shown = StaticText::builder(&panel).with_label("±10 %").build();
            tol.add(&s, 1, SizerFlag::Expand | SizerFlag::Right, 4);
            tol.add(&shown, 0, SizerFlag::AlignCenterVertical, 0);
            s.on_slider(move |e| shown.set_label(&format!("±{} %", e.get_value())));
            let both = BoxSizer::builder(Orientation::Horizontal).build();
            both.add(&t, 1, SizerFlag::Expand | SizerFlag::Right, 4);
            both.add_sizer(&tol, 1, SizerFlag::Expand, 0);
            grid.add_sizer(&both, 1, SizerFlag::Expand, 0);
            (c, t, s)
        };
        let (friction_on, friction, friction_tol) = band("Friction (Pa/m)", "1", "Pa/m", true);
        let (velocity_on, velocity, velocity_tol) = band("Velocity (m/s)", "5", "m/s", false);
        let aspect_on = CheckBox::builder(&panel).with_label("Aspect ratio 1:").build();
        grid.add(&aspect_on, 0, SizerFlag::AlignCenterVertical, 0);
        let aspect = text("3", "The largest long side over short side, for rectangular and oval ducts");
        grid.add(&aspect, 1, SizerFlag::Expand, 0);
        label("Roughness (mm)");
        let roughness = text("", "Empty: the type's (or 0.09 mm for duct, 0.0015 mm for pipe)");
        grid.add(&roughness, 1, SizerFlag::Expand, 0);
        label("Restrict H / W (mm)");
        let height = text("", "Rectangular: at most this high");
        let width = text("", "Rectangular: at most this wide");
        pair(&grid, &height, &width);
        label("Restrict Ø (mm)");
        let diameter = text("", "Round: at most this diameter");
        grid.add(&diameter, 1, SizerFlag::Expand, 0);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let show = Button::builder(&panel).with_label("Show Best Sizes").build();
        let swap = Button::builder(&panel).with_label("Swap Height/Width").build();
        let apply = Button::builder(&panel).with_label("Apply").build();
        apply.set_tooltip("Resize the selected duct or pipe to the selected size");
        for b in [&show, &swap, &apply] {
            buttons.add(b, 0, SizerFlag::Right, 4);
        }
        let list = ListCtrl::builder(&panel).with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel).build();
        for (k, (h, w)) in COLUMNS.iter().enumerate() {
            list.insert_column(k as i64, h, ListColumnFormat::Left, *w);
        }
        list.set_min_size(Size::new(300, 200));
        let note = StaticText::builder(&panel).with_label("").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&element_label, 0, SizerFlag::All, 6);
        sizer.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::All, 6);
        sizer.add(&note, 0, SizerFlag::Left | SizerFlag::Right, 6);
        sizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 6);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("sizing_calculator")
                .with_caption("Sizing Calculator")
                .right()
                .layer(2)
                .best_size(400, 700)
                .min_size(300, 360)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        let on = |b: &Button, f: fn(&mut Gui)| {
            let gui = gui.clone();
            b.on_click(move |_| with(&gui, f));
        };
        on(&show, Gui::calculator_show);
        on(&swap, |g| {
            if let Some(c) = &mut g.calculator {
                c.swap = !c.swap;
            }
            g.calculator_show();
        });
        on(&apply, Gui::calculator_apply);
        {
            let gui = gui.clone();
            list.on_column_click(move |e| {
                if let Some(k) = e.get_column() {
                    with(&gui, |g| g.calculator_sort(k.max(0) as usize));
                }
            });
        }
        with(gui, |g| {
            g.calculator = Some(SizingCalculator {
                element_label,
                flow,
                user_flow,
                domain,
                shape,
                friction_on,
                friction,
                friction_tol,
                velocity_on,
                velocity,
                velocity_tol,
                aspect_on,
                aspect,
                roughness,
                height,
                width,
                diameter,
                list,
                note,
                apply,
                rows: Vec::new(),
                swap: false,
                element: None,
                seen: None,
            });
            g.calculator_sync(true);
        });
    }

    /// Reads the selected segment into the palette when the selection has changed.
    pub(crate) fn calculator_sync(&mut self, force: bool) {
        if self.calculator.is_none() || !self.aui.is_pane_shown("sizing_calculator") {
            return;
        }
        let sel = self.selection();
        if !force && self.calculator.as_ref().and_then(|c| c.seen.as_ref()) == Some(&sel) {
            return;
        }
        let store = self.store_now();
        let one = match (&store, sel.as_slice()) {
            (Some(s), [id]) => s.get(*id).and_then(reviera_core::mep_runs::segment_recipe).filter(|r| matches!(r.domain, Domain::Duct | Domain::Pipe)).map(|r| (*id, r)),
            _ => None,
        };
        let read = one.as_ref().and_then(|(id, _)| self.run("sizing_calculator.candidates", json!({ "element_id": id, "flow": 1.0 })).ok());
        let flow = one.as_ref().and_then(|(id, _)| {
            let s = store.as_ref()?;
            let q = s.get(*id).and_then(|e| reviera_core::mep_network::kept_value(e, "flow")).and_then(serde_json::Value::as_f64);
            q.or_else(|| {
                reviera_core::mep_systems::systems(s)
                    .into_iter()
                    .filter(|x| x.elements.contains(id))
                    .find_map(|x| reviera_core::mep_sizing::segment_flows(s, x.id).ok()?.get(id).copied())
            })
        });
        let Some(c) = &mut self.calculator else { return };
        c.seen = Some(sel);
        match &one {
            Some((id, r)) => {
                let locked = if r.size_locked { ", size-locked" } else { "" };
                let from = match flow {
                    Some(q) if q > 0.0 => format!("{:.1} l/s from the model", q * 1000.0),
                    _ => "it carries no flow in the model: tick User value and type one".into(),
                };
                c.element_label.set_label(&format!("{} {}{locked}; {from}", r.domain.label(), r.size.text()));
                if !c.user_flow.get_value()
                    && let Some(q) = flow.filter(|q| *q > 0.0)
                {
                    c.flow.set_value(&format!("{:.1}", q * 1000.0));
                }
                c.domain.set_selection(if r.domain == Domain::Pipe { 1 } else { 0 });
                c.shape.set_selection(match r.size.shape {
                    Shape::Round => 0,
                    Shape::Rectangular => 1,
                    Shape::Oval => 2,
                });
                if let Some(x) = read.and_then(|v| serde_json::from_value::<CandidatesResult>(v).ok()) {
                    let mm = format!("{:.4}", x.roughness * 1000.0);
                    c.roughness.set_value(mm.trim_end_matches('0').trim_end_matches('.'));
                }
                c.element = Some((*id, r.domain));
            }
            None => {
                c.element_label.set_label("Calculator mode: select one duct or pipe to read and resize it");
                c.element = None;
            }
        }
        c.domain.enable(one.is_none());
        c.shape.enable(one.is_none());
        c.apply.enable(one.is_some());
    }

    /// Show Best Sizes.
    pub(crate) fn calculator_show(&mut self) {
        let frame = self.frame;
        let Some(c) = &self.calculator else { return };
        let band = |on: &CheckBox, t: &TextCtrl, s: &Slider| on.get_value().then(|| json!({ "target": num(t), "tolerance": s.get_value() as f64 / 100.0 }));
        let mm = |t: &TextCtrl| num(t).map(|v| v / 1000.0);
        let mut args = json!({
            "friction": band(&c.friction_on, &c.friction, &c.friction_tol),
            "velocity": band(&c.velocity_on, &c.velocity, &c.velocity_tol),
            "aspect": c.aspect_on.get_value().then(|| json!({ "max": num(&c.aspect).unwrap_or(3.0) })),
            "roughness": mm(&c.roughness),
            "restrict": { "height": mm(&c.height), "width": mm(&c.width), "inner_diameter": mm(&c.diameter) },
            "swap": c.swap,
        });
        let typed = num(&c.flow).map(|q| q / 1000.0);
        match c.element {
            Some((id, _)) => {
                args["element_id"] = json!(id);
                // The element's own flow unless User value is ticked.
                if c.user_flow.get_value() {
                    args["flow"] = json!(typed);
                }
            }
            None => {
                args["flow"] = json!(typed);
                args["domain"] = json!(if c.domain.get_selection() == Some(1) { "pipe" } else { "duct" });
                args["shape"] = json!(["round", "rectangular", "oval"][c.shape.get_selection().map_or(0, |k| k as usize).min(2)]);
            }
        }
        let r: CandidatesResult = match self.run("sizing_calculator.candidates", args) {
            Ok(v) => match serde_json::from_value(v) {
                Ok(r) => r,
                Err(e) => return error(&frame, &e.to_string()),
            },
            Err(e) => return error(&frame, &format!("No sizes:\n{e}")),
        };
        let Some(c) = &mut self.calculator else { return };
        c.note.set_label(&match &r.note {
            Some(n) => format!("{n}: the nearest sizes"),
            None => format!("{} size(s) within tolerance; the first is closest to the target", r.candidates.len()),
        });
        c.rows = r.candidates;
        self.calculator_fill(Some(0));
    }

    fn calculator_fill(&mut self, select: Option<usize>) {
        let Some(c) = &self.calculator else { return };
        c.list.delete_all_items();
        let grey = Colour::new(128, 128, 128, 255);
        for (k, row) in c.rows.iter().enumerate() {
            c.list.insert_item(k as i64, &cell(row, 0), None);
            for col in 1..COLUMNS.len() {
                c.list.set_item_text_by_column(k as i64, col as i32, &cell(row, col));
            }
            if !row.within {
                c.list.set_item_text_colour(k as i64, &grey);
            }
        }
        if let Some(k) = select.filter(|k| *k < c.rows.len()) {
            c.list.set_item_state(k as i64, ListItemState::Selected, ListItemState::Selected);
            c.list.ensure_visible(k as i64);
        }
    }

    /// Sorts the table by a column (again: the other way).
    pub(crate) fn calculator_sort(&mut self, k: usize) {
        let Some(c) = &mut self.calculator else { return };
        let ascending = c.rows.windows(2).all(|w| key(&w[0], k) <= key(&w[1], k));
        c.rows.sort_by(|a, b| key(a, k).total_cmp(&key(b, k)));
        if ascending {
            c.rows.reverse();
        }
        self.calculator_fill(None);
    }

    /// Apply: resizes the selected segment to the selected row.
    pub(crate) fn calculator_apply(&mut self) {
        let frame = self.frame;
        let Some(c) = &self.calculator else { return };
        let Some((id, domain)) = c.element else {
            return frame.set_status_text("Sizing Calculator: select one duct or pipe to apply a size", 0);
        };
        let k = c.list.get_first_selected_item();
        let Some(row) = usize::try_from(k).ok().and_then(|k| c.rows.get(k)).cloned() else {
            return frame.set_status_text("Sizing Calculator: choose a size in the table", 0);
        };
        let (command, args) = match (domain, row.shape) {
            (Domain::Pipe, _) => ("pipe.set_size", json!({ "element_ids": [id], "diameter": row.diameter })),
            (_, Shape::Round) => ("duct.set_size", json!({ "element_ids": [id], "diameter": row.diameter })),
            _ => ("duct.set_size", json!({ "element_ids": [id], "width": row.width, "height": row.height })),
        };
        match self.run(command, args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Resized to {}", row.size), 0);
                self.calculator_sync(true);
            }
            Err(e) => error(&frame, &format!("The size was not applied:\n{e}")),
        }
    }
}
