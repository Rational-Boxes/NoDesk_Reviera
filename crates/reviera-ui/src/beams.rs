// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Beams in the window (STR-BEAM-001, STR-BEAM-002): the Beam tool (Structure › Beam), On Grids,
//! Load Section for beam types, and a selected beam's Properties (level, end offsets, usage,
//! justification, orientation, rotation, extensions, its type's section).
//!
//! The tool works in plans: click a start and an end (snapping to grid crossings, wall lines and
//! the like); the beam's top is at the plan's level. It chains: each end starts the next beam,
//! until Esc.

use reviera_core::ElementId;
use reviera_core::beams::{self, Orientation, Usage, YJustification, ZJustification};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
pub(crate) const TYPE_KEY: &str = "IfcBeamType";
pub(crate) const BRACE_TYPE_KEY: &str = "IfcMemberType.brace";
const DUPLICATE: &str = "Duplicate Type…";

/// The Truss tool: its level and first click.
#[derive(Debug, Clone)]
pub struct TrussTool {
    level: ElementId,
    start: Option<[f64; 2]>,
}

const TRUSS_LAYOUTS: [(&str, &str); 6] = [
    ("Pratt", "pratt"),
    ("Howe", "howe"),
    ("Warren", "warren"),
    ("Fink", "fink"),
    ("King Post", "king_post"),
    ("Queen Post", "queen_post"),
];
const TRUSS_PROFILES: [(&str, &str); 6] = [
    ("Flat", "flat"),
    ("Gable", "gable"),
    ("Mono-pitch", "mono"),
    ("Scissor", "scissor"),
    ("Bowstring", "bowstring"),
    ("Custom", "custom"),
];
const BEARINGS: [(&str, &str); 2] = [("Bottom", "bottom"), ("Top", "top")];

/// The Beam System tool: its level.
#[derive(Debug, Clone)]
pub struct BeamSystemTool {
    level: ElementId,
}

const LAYOUT_RULES: [(&str, &str); 4] = [
    ("Maximum Spacing", "maximum_spacing"),
    ("Fixed Distance", "fixed_distance"),
    ("Fixed Number", "fixed_number"),
    ("Clear Spacing", "clear_spacing"),
];
const LAYOUT_JUST: [(&str, &str); 3] = [
    ("Beginning", "beginning"),
    ("End", "end"),
    ("Center", "centre"),
];

/// The Beam tool: its level and the chain's last point.
#[derive(Debug, Clone)]
pub struct BeamTool {
    level: ElementId,
    start: Option<[f64; 2]>,
    /// Structure › Brace: from the plan's level up to the next, not chained.
    brace: bool,
}

const USAGES: [(&str, Usage); 6] = [
    ("Automatic", Usage::Automatic),
    ("Girder", Usage::Girder),
    ("Joist", Usage::Joist),
    ("Purlin", Usage::Purlin),
    ("Horizontal Bracing", Usage::HorizontalBracing),
    ("Other", Usage::Other),
];
const Y_JUST: [(&str, YJustification); 4] = [
    ("Origin", YJustification::Origin),
    ("Left", YJustification::Left),
    ("Center", YJustification::Centre),
    ("Right", YJustification::Right),
];
const Z_JUST: [(&str, ZJustification); 4] = [
    ("Origin", ZJustification::Origin),
    ("Top", ZJustification::Top),
    ("Center", ZJustification::Centre),
    ("Bottom", ZJustification::Bottom),
];
const ORIENTATIONS: [(&str, Orientation); 2] = [
    ("Normal", Orientation::Normal),
    ("Horizontal", Orientation::Horizontal),
];

fn choices<T>(list: &[(&str, T)]) -> Vec<(String, i32)> {
    list.iter()
        .enumerate()
        .map(|(i, (n, _))| ((*n).to_owned(), i as i32))
        .collect()
}

fn index<T: PartialEq>(list: &[(&str, T)], v: &T) -> i32 {
    list.iter()
        .position(|(_, x)| x == v)
        .map_or(-1, |i| i as i32)
}

impl Gui {
    /// Structure › Beam (`brace` false) or Structure › Brace (true).
    pub(crate) fn start_beam_tool(&mut self, brace: bool) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Beams are drawn in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.beam_tool = Some(BeamTool {
            level,
            start: None,
            brace,
        });
        self.properties_stale = true;
        frame.set_status_text(
            if brace {
                "Brace: click its foot (on this level), then its head (on the level above); ends on columns and beams attach to them. Esc ends the tool."
            } else {
                "Beam: click its start, then its end (it snaps to grid crossings); each end starts the next. Esc ends the tool."
            },
            0,
        );
    }

    pub(crate) fn end_beam_tool(&mut self) -> bool {
        if self.beam_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// The type the tool draws: the one last chosen, else the first beam (or brace) type (none:
    /// the region's default, made by `beam.create` or `brace.create`).
    fn tool_beam_type(&self) -> Option<ElementId> {
        let store = self.store_now()?;
        let brace = self.beam_tool.as_ref().is_some_and(|t| t.brace);
        let class = if brace {
            "IfcMemberType"
        } else {
            "IfcBeamType"
        };
        let types: Vec<ElementId> = store
            .iter()
            .filter(|e| beams::beam_type(e).is_some() && e.class == class)
            .map(|e| e.id())
            .collect();
        self.insert_types
            .get(if brace { BRACE_TYPE_KEY } else { TYPE_KEY })
            .copied()
            .filter(|t| types.contains(t))
            .or_else(|| types.first().copied())
    }

    pub(crate) fn beam_point(&self, x: i32, y: i32, from: Option<[f64; 2]>) -> Option<[f64; 2]> {
        let lines = self.snap_lines();
        self.snapped_at(x, y, from, &lines).map(|s| s.point)
    }

    pub(crate) fn beam_motion(&mut self, x: i32, y: i32) {
        let Some(t) = self.beam_tool.clone() else {
            return;
        };
        let (Some(a), Some(b)) = (t.start, self.beam_point(x, y, t.start)) else {
            return;
        };
        let width = self
            .tool_beam_type()
            .and_then(|ty| self.store_now()?.get(ty).and_then(beams::beam_type))
            .map_or(0.15, |ty| ty.width);
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |pl| pl.range.cut) as f32
            + 0.004;
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
        let n = [
            -(b[1] - a[1]) / l * width / 2.0,
            (b[0] - a[0]) / l * width / 2.0,
        ];
        let p = |q: [f64; 2], k: f64| [(q[0] + n[0] * k) as f32, (q[1] + n[1] * k) as f32, z];
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: vec![
                [p(a, 1.0), p(b, 1.0)],
                [p(a, -1.0), p(b, -1.0)],
                [p(a, 1.0), p(a, -1.0)],
                [p(b, 1.0), p(b, -1.0)],
            ],
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    pub(crate) fn beam_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.beam_tool.clone() else {
            return;
        };
        let Some(p) = self.beam_point(x, y, t.start) else {
            return;
        };
        let Some(a) = t.start else {
            if let Some(t) = &mut self.beam_tool {
                t.start = Some(p);
            }
            return frame.set_status_text("Beam: click its end", 0);
        };
        if (p[0] - a[0]).hypot(p[1] - a[1]) < 1e-3 {
            return;
        }
        let (command, mut args) = if t.brace {
            // Up to the next level, else 3 m up.
            let next = self.store_now().and_then(|st| {
                let z = reviera_core::levels::elevation(st.get(t.level)?);
                reviera_core::levels::list(&st)
                    .into_iter()
                    .find(|l| l.elevation > z + 1e-6)
                    .map(|l| l.id)
            });
            let (end_level, end_offset) = match next {
                Some(l) => (l, 0.0),
                None => (t.level, 3.0),
            };
            (
                "brace.create",
                json!({
                    "start_point": a, "start_level": t.level,
                    "end_point": p, "end_level": end_level, "end_offset": end_offset,
                }),
            )
        } else {
            (
                "beam.create",
                json!({ "curve": { "start": a, "end": p }, "reference_level": t.level }),
            )
        };
        if let Some(ty) = self.tool_beam_type() {
            args["type_id"] = json!(ty);
        }
        match self.run(command, args) {
            Ok(v) => {
                if let Some(t) = &mut self.beam_tool {
                    // Beams chain; each brace starts afresh.
                    t.start = (!t.brace).then_some(p);
                }
                let usage = self
                    .run("beam.info", json!({ "beam_ids": [v] }))
                    .ok()
                    .and_then(|i| i[0]["usage"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                frame.set_status_text(
                    &if t.brace {
                        format!("Brace placed ({usage}); click the next brace's foot, or Esc")
                    } else {
                        format!("Beam placed ({usage}); click the next end, or Esc")
                    },
                    0,
                );
                self.refresh_scene(&frame);
                self.properties_stale = true;
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The beam type selector (`bt_type`) and its section rows (`ct_`, shared with columns).
    fn beam_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>, brace: bool) {
        let Some(store) = self.store_now() else {
            return;
        };
        let class = if brace {
            "IfcMemberType"
        } else {
            "IfcBeamType"
        };
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| beams::beam_type(e).is_some() && e.class == class)
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut list: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        list.push((DUPLICATE.to_owned(), list.len() as i32));
        let at = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_beam_type"));
        grid.append(Property::enumeration("Type", "bt_type", list, at));
        self.type_rows = current;
        if let Some(t) = current
            .and_then(|c| store.get(c))
            .and_then(beams::beam_type)
        {
            self.section_rows(grid, &t);
        }
    }

    /// The palette while the Beam tool runs.
    pub(crate) fn beam_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.beam_tool.is_none() {
            return false;
        }
        grid.append(Property::category("Beam tool", "c_beamtool"));
        let current = self.tool_beam_type();
        let brace = self.beam_tool.as_ref().is_some_and(|t| t.brace);
        self.beam_type_rows(grid, current, brace);
        true
    }

    /// The rows of a selected beam; `false` if `id` is not one.
    pub(crate) fn beam_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = beams::beam_recipe(e) else {
            return false;
        };
        let info = self
            .run("beam.info", json!({ "beam_ids": [id] }))
            .ok()
            .map(|v| v[0].clone())
            .unwrap_or_default();
        grid.append(Property::category("Constraints", "c_beam"));
        let levels = reviera_core::levels::list(&store);
        let names: Vec<(String, i32)> = levels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i as i32))
            .collect();
        let at = levels
            .iter()
            .position(|l| {
                store
                    .get(l.id)
                    .is_some_and(|e| e.guid().to_string() == r.level)
            })
            .map_or(-1, |i| i as i32);
        grid.append(Property::enumeration(
            "Reference Level",
            "bm_level",
            names,
            at,
        ));
        let v = self.format_length(r.start_offset);
        grid.append(Property::string("Start Level Offset", "bm_start_off", v));
        if r.brace {
            let names: Vec<(String, i32)> = levels
                .iter()
                .enumerate()
                .map(|(i, l)| (l.name.clone(), i as i32))
                .collect();
            let end = r.end_level.as_ref().unwrap_or(&r.level);
            let at = levels
                .iter()
                .position(|l| {
                    store
                        .get(l.id)
                        .is_some_and(|e| e.guid().to_string() == *end)
                })
                .map_or(-1, |i| i as i32);
            grid.append(Property::enumeration(
                "End Level",
                "bm_end_level",
                names,
                at,
            ));
            let attached = |k: usize| {
                if r.attach[k].is_some() {
                    "Attached"
                } else {
                    "Free"
                }
            };
            let s = format!("{} / {}", attached(0), attached(1));
            grid.append(Property::string("Start / End Attachment", "bm_attach", s));
            grid.set_property_read_only("bm_attach", true);
        }
        let v = self.format_length(r.end_offset);
        grid.append(Property::string("End Level Offset", "bm_end_off", v));
        grid.append(Property::enumeration(
            "Orientation",
            "bm_orientation",
            choices(&ORIENTATIONS),
            index(&ORIENTATIONS, &r.orientation),
        ));
        let degrees = format!("{:.1}°", r.rotation.to_degrees());
        grid.append(Property::string(
            "Cross-Section Rotation",
            "bm_rotation",
            degrees,
        ));
        grid.append(Property::category("Geometric Position", "c_beam_pos"));
        grid.append(Property::enumeration(
            "y Justification",
            "bm_yj",
            choices(&Y_JUST),
            index(&Y_JUST, &r.y_justification),
        ));
        let v = self.format_length(r.y_offset);
        grid.append(Property::string("y Offset Value", "bm_yo", v));
        grid.append(Property::enumeration(
            "z Justification",
            "bm_zj",
            choices(&Z_JUST),
            index(&Z_JUST, &r.z_justification),
        ));
        let v = self.format_length(r.z_offset);
        grid.append(Property::string("z Offset Value", "bm_zo", v));
        let v = self.format_length(r.start_extension);
        grid.append(Property::string("Start Extension", "bm_start_ext", v));
        let v = self.format_length(r.end_extension);
        grid.append(Property::string("End Extension", "bm_end_ext", v));
        grid.append(Property::category("Structural", "c_beam_str"));
        // Automatic shows what it resolves to.
        let mut usages = choices(&USAGES);
        if r.usage == Usage::Automatic {
            let resolved = USAGES
                .iter()
                .find(|(_, u)| serde_json::to_value(u).ok().as_ref() == Some(&info["usage"]))
                .map_or("", |(n, _)| *n);
            usages[0].0 = format!("Automatic ({resolved})");
        }
        grid.append(Property::enumeration(
            "Structural Usage",
            "bm_usage",
            usages,
            index(&USAGES, &r.usage),
        ));
        // A brace's usage follows its ends.
        grid.set_property_read_only("bm_usage", r.brace);
        let supports = |k: usize| {
            info["supports"][k][0].as_str().map_or("—".to_owned(), |s| {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            })
        };
        let s = format!("{} / {}", supports(0), supports(1));
        grid.append(Property::string(
            "Supported At Start / End",
            "bm_supports",
            s,
        ));
        grid.set_property_read_only("bm_supports", true);
        let v = self.format_length(info["length"].as_f64().unwrap_or(0.0));
        grid.append(Property::string("Cut Length", "bm_length", v));
        grid.set_property_read_only("bm_length", true);
        // A member of a truss: the truss's layout (STR-TRUSS-001).
        if let Some(t) = r
            .system
            .as_ref()
            .and_then(|s| reviera_core::GlobalId::parse(&s.system))
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|s| store.get(s))
            .and_then(beams::truss_recipe)
        {
            grid.append(Property::category("Truss", "c_truss"));
            let pick = |list: &[(&str, &str)], v: serde_json::Value| {
                list.iter()
                    .position(|(_, k)| v == *k)
                    .map_or(-1, |i| i as i32)
            };
            grid.append(Property::enumeration(
                "Layout",
                "tr_layout",
                choices(&TRUSS_LAYOUTS),
                pick(
                    &TRUSS_LAYOUTS,
                    serde_json::to_value(t.layout).unwrap_or_default(),
                ),
            ));
            grid.append(Property::string(
                "Panels",
                "tr_panels",
                t.panels.to_string(),
            ));
            let h = self.format_length(t.height);
            grid.append(Property::string("Truss Height", "tr_height", h));
            grid.append(Property::enumeration(
                "Profile",
                "tr_profile",
                choices(&TRUSS_PROFILES),
                pick(
                    &TRUSS_PROFILES,
                    serde_json::to_value(t.profile).unwrap_or_default(),
                ),
            ));
            let heel = self.format_length(t.heel);
            grid.append(Property::string("Heel Height", "tr_heel", heel));
            grid.append(Property::enumeration(
                "Bearing Chord",
                "tr_bearing",
                choices(&BEARINGS),
                pick(
                    &BEARINGS,
                    serde_json::to_value(t.bearing).unwrap_or_default(),
                ),
            ));
        }
        // A beam of a beam system: the system's layout (STR-BSYS-001).
        if let Some(sys) = r
            .system
            .as_ref()
            .and_then(|s| reviera_core::GlobalId::parse(&s.system))
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|s| store.get(s))
            .and_then(beams::system_recipe)
        {
            grid.append(Property::category("Beam System", "c_beam_sys"));
            let rule = serde_json::to_value(sys.rule).unwrap_or_default();
            let at = LAYOUT_RULES
                .iter()
                .position(|(_, k)| rule == *k)
                .map_or(-1, |i| i as i32);
            grid.append(Property::enumeration(
                "Layout Rule",
                "bs_rule",
                choices(&LAYOUT_RULES),
                at,
            ));
            let v = if sys.rule == beams::LayoutRule::FixedNumber {
                format!("{}", sys.value.round())
            } else {
                self.format_length(sys.value)
            };
            grid.append(Property::string(
                if sys.rule == beams::LayoutRule::FixedNumber {
                    "Number of Lines"
                } else {
                    "Spacing"
                },
                "bs_value",
                v,
            ));
            let j = serde_json::to_value(sys.justification).unwrap_or_default();
            let at = LAYOUT_JUST
                .iter()
                .position(|(_, k)| j == *k)
                .map_or(-1, |i| i as i32);
            grid.append(Property::enumeration(
                "Justification",
                "bs_just",
                choices(&LAYOUT_JUST),
                at,
            ));
        }
        let current = e
            .param("@type")
            .and_then(serde_json::Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        self.beam_type_rows(grid, current, r.brace);
        true
    }

    /// The beam systems of the selected beams.
    fn selected_systems(&mut self) -> Vec<ElementId> {
        let ids = self.selected_beams();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        let mut out: Vec<ElementId> = ids
            .iter()
            .filter_map(|b| beams::beam_recipe(store.get(*b)?)?.system)
            .filter_map(|s| store.find_by_guid(&reviera_core::GlobalId::parse(&s.system)?))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The trusses of the selected members.
    fn selected_trusses(&mut self) -> Vec<ElementId> {
        let ids = self.selected_beams();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        let mut out: Vec<ElementId> = ids
            .iter()
            .filter_map(|b| beams::beam_recipe(store.get(*b)?)?.system)
            .filter_map(|s| store.find_by_guid(&reviera_core::GlobalId::parse(&s.system)?))
            .filter(|t| store.get(*t).and_then(beams::truss_recipe).is_some())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Structure › Truss: click its start and end on this plan's level.
    pub(crate) fn start_truss_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Trusses are drawn in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.truss_tool = Some(TrussTool { level, start: None });
        frame.set_status_text(
            "Truss: click its start, then its end (its bottom chord at this level; Pratt, a panel about every 1.5 m). Esc ends the tool.",
            0,
        );
    }

    pub(crate) fn end_truss_tool(&mut self) -> bool {
        self.truss_tool.take().is_some()
    }

    pub(crate) fn truss_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.truss_tool.clone() else {
            return;
        };
        let Some(p) = self.beam_point(x, y, t.start) else {
            return;
        };
        let Some(a) = t.start else {
            if let Some(t) = &mut self.truss_tool {
                t.start = Some(p);
            }
            return frame.set_status_text("Truss: click its end", 0);
        };
        match self.run(
            "truss.create",
            json!({ "start": a, "end": p, "level": t.level }),
        ) {
            Ok(_) => {
                if let Some(t) = &mut self.truss_tool {
                    t.start = None;
                }
                frame.set_status_text("Truss placed; click the next truss's start, or Esc", 0);
                self.refresh_scene(&frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Modify › Remove Truss: the selected members' trusses go; their members stay.
    pub(crate) fn remove_trusses(&mut self) {
        let frame = self.frame;
        let trusses = self.selected_trusses();
        if trusses.is_empty() {
            return frame.set_status_text("Select a member of a truss first", 0);
        }
        for t in trusses {
            if let Err(e) = self.run("truss.remove", json!({ "truss_id": t })) {
                error(&frame, &format!("The truss was not removed:\n{e}"));
            }
        }
        frame.set_status_text("Truss removed; its members stay", 0);
        self.properties_stale = true;
        self.refresh_scene(&frame);
    }

    /// Structure › Beam System: click in a bay of beams or walls on this plan's level.
    pub(crate) fn start_beam_system_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Beam systems are placed in plan views", 0);
        };
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.beam_system_tool = Some(BeamSystemTool { level });
        frame.set_status_text(
            "Beam System: click inside a bay of beams or walls; beams run parallel to the nearest support. Esc ends the tool.",
            0,
        );
    }

    pub(crate) fn end_beam_system_tool(&mut self) -> bool {
        self.beam_system_tool.take().is_some()
    }

    pub(crate) fn beam_system_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.beam_system_tool.clone() else {
            return;
        };
        let Some(p) = self.plan_xy(x, y) else { return };
        // 4'-0" in US models, else 1200 mm.
        let imperial = self.store_now().is_some_and(|s| {
            reviera_core::codes::settings(&s).code.structural().region
                == reviera_core::codes::Region::UnitedStates
        });
        let mut args = json!({
            "point": p, "level": t.level, "layout_rule": "maximum_spacing",
            "value": if imperial { 1.2192 } else { 1.2 },
        });
        if let Some(ty) = self.tool_beam_type() {
            args["type_id"] = json!(ty);
        }
        match self.run("beam_system.create_auto", args) {
            Ok(v) => {
                let n = self
                    .run("beam_system.info", json!({ "system_id": v }))
                    .ok()
                    .and_then(|i| i["beams"].as_array().map(Vec::len))
                    .unwrap_or(0);
                frame.set_status_text(
                    &format!("Beam system placed ({n} beams); click another bay, or Esc"),
                    0,
                );
                self.refresh_scene(&frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Modify › Remove Beam System: the selected beams' systems go; their beams stay.
    pub(crate) fn remove_beam_systems(&mut self) {
        let frame = self.frame;
        let systems = self.selected_systems();
        if systems.is_empty() {
            return frame.set_status_text("Select a beam of a beam system first", 0);
        }
        for s in systems {
            if let Err(e) = self.run("beam_system.remove", json!({ "system_id": s })) {
                error(&frame, &format!("The beam system was not removed:\n{e}"));
            }
        }
        frame.set_status_text("Beam system removed; its beams stay", 0);
        self.properties_stale = true;
        self.refresh_scene(&frame);
    }

    fn selected_beams(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).and_then(beams::beam_recipe).is_some())
            .collect()
    }

    /// An edit of a beam row; `false` if `property` is not one.
    pub(crate) fn beam_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if property.starts_with("bs_") {
            return self.beam_system_edit(property, text);
        }
        if property.starts_with("tr_") {
            let frame = self.frame;
            let text = text.unwrap_or_default();
            let args = match property {
                "tr_layout" => TRUSS_LAYOUTS
                    .iter()
                    .find(|(n, _)| *n == text)
                    .map(|(_, k)| json!({ "layout": k })),
                "tr_bearing" => BEARINGS
                    .iter()
                    .find(|(n, _)| *n == text)
                    .map(|(_, k)| json!({ "bearing": k })),
                "tr_panels" => text
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .map(|n| json!({ "panels": n })),
                "tr_height" => Some(json!({ "height": text })),
                "tr_heel" => Some(json!({ "heel": text })),
                "tr_profile" => TRUSS_PROFILES
                    .iter()
                    .find(|(n, _)| *n == text)
                    .filter(|(_, k)| *k != "custom")
                    .map(|(_, k)| json!({ "profile": k })),
                _ => None,
            };
            if let Some(mut a) = args {
                for t in self.selected_trusses() {
                    a["truss_id"] = json!(t);
                    if let Err(e) = self.run("truss.set", a.clone()) {
                        error(&frame, &format!("The change was not made:\n{e}"));
                    }
                }
            }
            self.clear_selection();
            self.properties_stale = true;
            self.refresh_scene(&frame);
            return true;
        }
        if !property.starts_with("bm_") && property != "bt_type" {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected = self.selected_beams();
        let Some(store) = self.store_now() else {
            return true;
        };
        let set = |g: &mut Gui, mut args: serde_json::Value| -> Result<(), String> {
            args["beam_ids"] = json!(selected);
            g.run("beam.set", args).map(|_| ())
        };
        let pick = |list: Vec<(&str, serde_json::Value)>| {
            list.into_iter()
                .find(|(n, _)| text.starts_with(n))
                .map(|(_, v)| v)
        };
        let result: Option<Result<(), String>> = match property {
            "bm_level" => reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == text)
                .map(|l| set(self, json!({ "reference_level": l.id }))),
            "bm_start_off" => Some(set(self, json!({ "start_offset": text }))),
            "bm_end_level" => reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == text)
                .map(|l| set(self, json!({ "end_level": l.id }))),
            "bm_end_off" => Some(set(self, json!({ "end_offset": text }))),
            "bm_yo" => Some(set(self, json!({ "y_offset": text }))),
            "bm_zo" => Some(set(self, json!({ "z_offset": text }))),
            "bm_start_ext" => Some(set(self, json!({ "start_extension": text }))),
            "bm_end_ext" => Some(set(self, json!({ "end_extension": text }))),
            "bm_rotation" => {
                let t = if text.trim().ends_with('°') || text.contains("deg") {
                    text.clone()
                } else {
                    format!("{}°", text.trim())
                };
                Some(set(self, json!({ "rotation": t })))
            }
            "bm_orientation" => pick(ORIENTATIONS.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "orientation": v }))),
            "bm_yj" => pick(Y_JUST.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "y_justification": v }))),
            "bm_zj" => pick(Z_JUST.iter().map(|(n, v)| (*n, json!(v))).collect())
                .map(|v| set(self, json!({ "z_justification": v }))),
            "bm_usage" => pick(USAGES.iter().map(|(n, v)| (*n, json!(v))).collect()).map(|v| {
                self.run(
                    "beam.set_usage",
                    json!({ "beam_ids": selected, "usage": v }),
                )
                .map(|_| ())
            }),
            "bt_type" => self.choose_beam_type(&text, &selected),
            _ => None,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// An edit of a beam system row (on one of its beams).
    fn beam_system_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let args = match property {
            "bs_rule" => LAYOUT_RULES
                .iter()
                .find(|(n, _)| *n == text)
                .map(|(_, k)| json!({ "rule": k })),
            "bs_just" => LAYOUT_JUST
                .iter()
                .find(|(n, _)| *n == text)
                .map(|(_, k)| json!({ "justification": k })),
            "bs_value" => Some(match text.trim().parse::<f64>() {
                // A count, or a length in the display unit.
                Ok(n) if !text.contains(['m', '\'', '"']) && n.fract() == 0.0 && n < 50.0 => {
                    json!({ "value": n })
                }
                _ => json!({ "value": text }),
            }),
            _ => None,
        };
        if let Some(mut a) = args {
            for s in self.selected_systems() {
                a["system_id"] = json!(s);
                if let Err(e) = self.run("beam_system.set_layout", a.clone()) {
                    error(&frame, &format!("The change was not made:\n{e}"));
                }
            }
        }
        self.clear_selection();
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    fn choose_beam_type(
        &mut self,
        choice: &str,
        selected: &[ElementId],
    ) -> Option<Result<(), String>> {
        let frame = self.frame;
        let store = self.store_now()?;
        let chosen = if choice == DUPLICATE {
            let current = self.type_rows?;
            let proposed = reviera_core::types::duplicate_name(&store, current);
            let dialog =
                TextEntryDialog::builder(&frame, "Name of the new type:", "Duplicate Type")
                    .with_default_value(&proposed)
                    .build();
            if dialog.show_modal() != ID_OK {
                return None;
            }
            let name = dialog.get_value()?;
            match self.run(
                "type.duplicate",
                json!({ "type_id": current, "new_name": name }),
            ) {
                Ok(v) => ElementId(v.as_u64().unwrap_or_default()),
                Err(e) => return Some(Err(e)),
            }
        } else {
            store
                .iter()
                .find(|e| beams::beam_type(e).is_some() && e.name.as_deref() == Some(choice))?
                .id()
        };
        if selected.is_empty() {
            let brace = self.beam_tool.as_ref().is_some_and(|t| t.brace);
            self.insert_types
                .insert(if brace { BRACE_TYPE_KEY } else { TYPE_KEY }, chosen);
            return Some(Ok(()));
        }
        Some(
            self.run(
                "element.set_type",
                json!({ "element_ids": selected, "type_id": chosen }),
            )
            .map(|_| ()),
        )
    }

    /// Structure › Beams on Grids: a beam in each bay between columns along the selected grid
    /// lines, at the plan's level.
    pub(crate) fn beams_on_grids(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let grids: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| store.get(*id).is_some_and(reviera_core::grids::is_grid))
            .collect();
        if grids.is_empty() {
            return frame.set_status_text(
                "Select the grid lines first (drag a window or Ctrl+click), then Beams on Grids",
                0,
            );
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Beams on Grids works in plan views", 0);
        };
        let mut args = json!({ "grid_ids": grids, "level": level });
        if let Some(t) = self.tool_beam_type() {
            args["type_id"] = json!(t);
        }
        match self.run("beam.create_on_grids", args) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} beams placed between columns"), 0);
                self.clear_selection();
                self.refresh_scene(&frame);
            }
            Err(e) => error(&frame, &format!("The beams were not placed:\n{e}")),
        }
    }
}
