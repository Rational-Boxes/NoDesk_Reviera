// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Array along Path in the window (ARC-MOD-033): the tool, a selected array's Properties and
//! Unarray.
//!
//! Select a component (a placed custom object), or pick one up in the Component Library, and
//! choose Modify › Array along Path (AP). Then either click a wall, a grid line or a floor
//! (ceiling, deck) to follow its line or its edges, or click points in a plan to draw a path and
//! press Enter (Properties: polyline, spline or arc; closed). The distribution, corners and
//! orientation are in Properties while the tool runs and on the array afterwards; every change
//! regenerates the copies.

use reviera_core::ElementId;
use reviera_core::path_arrays::{self, Corners, Mode, Orientation, PathSpec, Shape};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::components::Source;
use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

const MODES: [(Mode, &str); 4] = [
    (Mode::Number, "Number"),
    (Mode::Fixed, "Fixed Distance"),
    (Mode::Max, "Maximum Spacing"),
    (Mode::Min, "Minimum Spacing"),
];
const CORNERS: [(Corners, &str); 3] = [
    (Corners::Through, "Spaced through corners"),
    (Corners::EveryCorner, "At every corner"),
    (Corners::Skip, "Corners left empty"),
];
const ORIENTATIONS: [(Orientation, &str); 4] = [
    (Orientation::Follow, "Follow path"),
    (Orientation::Fixed, "Fixed"),
    (Orientation::Outward, "Face outward"),
    (Orientation::Inward, "Face inward"),
];
const SHAPES: [(Shape, &str); 3] = [
    (Shape::Polyline, "Polyline"),
    (Shape::Spline, "Spline"),
    (Shape::Arc, "Arc (3 points)"),
];

/// The Array along Path tool.
#[derive(Debug, Clone)]
pub struct ArrayTool {
    /// What to repeat: `{type_id}` or `{library, item}`.
    source: Value,
    name: String,
    level: Option<ElementId>,
    points: Vec<[f64; 2]>,
    cursor: Option<[f64; 2]>,
    shape: Shape,
    closed: bool,
    mode: Mode,
    value: f64,
    counts: Vec<u32>,
    corners: Corners,
    orientation: Orientation,
}

fn label<T: PartialEq + Copy>(table: &[(T, &'static str)], v: T) -> i32 {
    table.iter().position(|x| x.0 == v).unwrap_or(0) as i32
}

fn from_label<T: Copy>(table: &[(T, &'static str)], text: &str) -> Option<T> {
    table.iter().find(|x| x.1 == text).map(|x| x.0)
}

fn choices<T>(table: &[(T, &'static str)]) -> Vec<(String, i32)> {
    table
        .iter()
        .enumerate()
        .map(|(i, x)| (x.1.to_owned(), i as i32))
        .collect()
}

fn parse_counts(text: &str) -> Vec<u32> {
    text.split([',', ' ', ';'])
        .filter_map(|t| t.trim().parse().ok())
        .collect()
}

impl Gui {
    /// Modify › Array along Path.
    pub(crate) fn start_array_tool(&mut self) {
        let frame = self.frame;
        if self.doc.is_none() {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        }
        let store = self.store_now();
        // The selected custom object's type, else what Place Component holds.
        let picked = self.selection().into_iter().find_map(|id| {
            let s = store.as_ref()?;
            let (t, _) = reviera_core::mesh_objects::type_of(s, s.get(id)?)?;
            Some((
                json!({ "type_id": t }),
                s.get(t)?.name.clone().unwrap_or_default(),
            ))
        });
        let held = self.place_tool.as_ref().and_then(|t| {
            let source = match &t.source {
                Source::Type(ty) => json!({ "type_id": ty }),
                Source::Item { library, item } => json!({ "library": library, "item": item }),
                // Path arrays of fragments are not built yet.
                Source::Fragment { .. } => return None,
            };
            Some((source, t.name.clone()))
        });
        let Some((source, name)) = picked.or(held) else {
            return error(
                &frame,
                "Select a placed component (a custom object), or pick one in the Component Library, then Array along Path.",
            );
        };
        let level = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
            .or_else(|| {
                store
                    .as_ref()
                    .and_then(|s| reviera_core::levels::list(s).first().map(|l| l.id))
            });
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.array_tool = Some(ArrayTool {
            source,
            name: name.clone(),
            level,
            points: Vec::new(),
            cursor: None,
            shape: Shape::Polyline,
            closed: false,
            mode: Mode::Number,
            value: 6.0,
            counts: Vec::new(),
            corners: Corners::Through,
            orientation: Orientation::Follow,
        });
        self.properties_stale = true;
        frame.set_status_text(
            &format!(
                "Array {name} along Path: click a wall, grid line or floor to follow it, or click points to draw a path (Enter finishes). Esc cancels."
            ),
            0,
        );
    }

    pub(crate) fn end_array_tool(&mut self) -> bool {
        if self.array_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// A path point: snapped in a plan; in 3D on the surface under the cursor (or the ground).
    fn array_point(&self, x: i32, y: i32) -> Option<[f64; 2]> {
        self.place_at(x, y).map(|(p, _, _)| p)
    }

    pub(crate) fn array_motion(&mut self, x: i32, y: i32) {
        let p = self.array_point(x, y);
        let z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or(0.0, |pl| pl.range.cut)
            + 0.004;
        let Some(t) = &mut self.array_tool else {
            return;
        };
        t.cursor = p;
        let mut pts = t.points.clone();
        if let Some(c) = p {
            pts.push(c);
        }
        if t.closed && pts.len() > 2 {
            pts.push(pts[0]);
        }
        let segments = pts
            .windows(2)
            .map(|w| {
                [
                    [w[0][0] as f32, w[0][1] as f32, z as f32],
                    [w[1][0] as f32, w[1][1] as f32, z as f32],
                ]
            })
            .collect();
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[Lines {
                color: PREVIEW,
                width: 0.5,
                segments,
            }]);
        }
    }

    pub(crate) fn array_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(t) = &self.array_tool else { return };
        // The first click on a wall, grid line or floor follows it.
        if t.points.is_empty()
            && let Some(hit) = self.element_at(x, y, panel)
            && let Some(store) = self.store_now()
            && let Some(e) = store.get(hit)
        {
            let path = if reviera_core::walls::wall_recipe(e).is_some()
                || reviera_core::grids::grid_data(e).is_some()
            {
                Some(json!({ "element": hit }))
            } else if reviera_core::sketch::sketch_of(e).is_some()
                && reviera_core::floors::slab_recipe(e).is_some()
            {
                Some(json!({ "host_edges": hit }))
            } else {
                None
            };
            if let Some(path) = path {
                return self.make_array(path);
            }
        }
        let Some(p) = self.array_point(x, y) else {
            return frame.set_status_text(
                "Array along Path: click points for the path, or click a wall, grid or floor",
                0,
            );
        };
        let Some(t) = &mut self.array_tool else {
            return;
        };
        t.points.push(p);
        let n = t.points.len();
        let shape = t.shape;
        if shape == Shape::Arc && n == 3 {
            return self.finish_array_path();
        }
        frame.set_status_text(
            &format!(
                "Array along Path: {n} point{} — click more, Enter finishes",
                if n == 1 { "" } else { "s" }
            ),
            0,
        );
        self.array_motion(x, y);
    }

    /// Enter: makes the array along the drawn points.
    pub(crate) fn array_key(&mut self, key: i32) -> bool {
        if self.array_tool.is_none() || !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            return false;
        }
        self.finish_array_path();
        true
    }

    fn finish_array_path(&mut self) {
        let Some(t) = &self.array_tool else { return };
        if t.points.len() < 2 {
            return self
                .frame
                .set_status_text("Array along Path: click two points or more first", 0);
        }
        let path = json!({ "points": t.points, "shape": t.shape, "closed": t.closed });
        self.make_array(path);
    }

    fn make_array(&mut self, path: Value) {
        let frame = self.frame;
        let Some(t) = self.array_tool.clone() else {
            return;
        };
        let Some(level) = t.level else { return };
        let mut distribution = json!({ "mode": t.mode, "value": t.value });
        if !t.counts.is_empty() {
            distribution["counts"] = json!(t.counts);
        }
        let args = json!({
            "sources": [t.source], "path": path, "level": level, "distribution": distribution,
            "corners": t.corners, "orientation": t.orientation,
            "name": format!("{} Array", t.name),
        });
        match self.run("array.along_path", args) {
            Ok(v) => {
                self.end_array_tool();
                self.refresh_scene(&frame);
                if let Some(id) = v.as_u64() {
                    self.select_ids(&[ElementId(id)], super::select::Combine::Replace);
                }
                frame.set_status_text(
                    "Array made: change its count, spacing and corners in Properties",
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The array was not made:\n{e}")),
        }
    }

    /// The palette while the tool runs.
    pub(crate) fn array_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(t) = self.array_tool.clone() else {
            return false;
        };
        grid.append(Property::category("Array along Path", "c_arrtool"));
        grid.append(Property::string("Component", "at_name", t.name.clone()));
        grid.set_property_read_only("at_name", true);
        grid.append(Property::enumeration(
            "Path Shape",
            "at_shape",
            choices(&SHAPES),
            label(&SHAPES, t.shape),
        ));
        grid.append(Property::boolean("Closed", "at_closed", t.closed));
        distribution_rows(
            grid,
            "at",
            t.mode,
            t.value,
            &t.counts,
            t.corners,
            t.orientation,
        );
        true
    }

    /// The rows of a selected path array; `false` if `id` is not one.
    pub(crate) fn path_array_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(r) = store.get(id).and_then(path_arrays::path_array_recipe) else {
            return false;
        };
        let members = path_arrays::members(&store, &r)
            .map(|m| m.len())
            .unwrap_or(0);
        grid.append(Property::category("Array along Path", "c_pa"));
        grid.append(Property::string("Copies", "pa_copies", members.to_string()));
        grid.set_property_read_only("pa_copies", true);
        let path = match &r.path {
            PathSpec::Drawn {
                points,
                shape,
                closed,
            } => format!(
                "Drawn {} of {} points{}",
                SHAPES
                    .iter()
                    .find(|x| x.0 == *shape)
                    .map_or("path", |x| x.1)
                    .to_lowercase(),
                points.len(),
                if *closed { ", closed" } else { "" }
            ),
            PathSpec::Element { .. } => "Follows a wall or grid line".to_owned(),
            PathSpec::HostEdges { .. } => "Follows a host's edges".to_owned(),
        };
        grid.append(Property::string("Path", "pa_path", path));
        grid.set_property_read_only("pa_path", true);
        distribution_rows(
            grid,
            "pa",
            r.distribution.mode,
            r.distribution.value,
            &r.distribution.counts,
            r.corners,
            r.orientation,
        );
        grid.append(Property::boolean(
            "Include Start",
            "pa_include_start",
            r.include_start,
        ));
        grid.append(Property::boolean(
            "Include End",
            "pa_include_end",
            r.include_end,
        ));
        let off = self.format_length(r.offset);
        grid.append(Property::string("Offset", "pa_offset", off));
        grid.append(Property::string(
            "Rotation Offset",
            "pa_rotation_offset",
            format!("{:.1}°", r.rotation_offset.to_degrees()),
        ));
        grid.append(Property::category("Variation", "c_pa_var"));
        let range = |v: Option<[f64; 2]>, f: &dyn Fn(f64) -> String| {
            v.map_or(String::new(), |[a, b]| format!("{} to {}", f(a), f(b)))
        };
        grid.append(Property::string(
            "Random Turn",
            "pa_var_rotation",
            range(r.variation.rotation, &|v| format!("{:.0}°", v.to_degrees())),
        ));
        grid.set_help_string("pa_var_rotation", "Empty for none; for example 0 to 360");
        grid.append(Property::string(
            "Random Scale",
            "pa_var_scale",
            range(r.variation.scale, &|v| format!("{v}")),
        ));
        grid.set_help_string("pa_var_scale", "Empty for none; for example 0.85 to 1.15");
        grid.append(Property::boolean("Reseed", "pa_reseed", false));
        grid.set_help_string("pa_reseed", "Tick for a new random arrangement");
        grid.append(Property::string(
            "Excluded",
            "pa_excluded",
            r.excluded.len().to_string(),
        ));
        grid.set_help_string(
            "pa_excluded",
            "Members left out; type 0 to restore them all",
        );
        true
    }

    /// An edit of the tool's or a selected array's row; `false` if `property` is not one.
    pub(crate) fn path_array_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let text = text.unwrap_or_default();
        if let Some(row) = property.strip_prefix("at_") {
            let Some(t) = &mut self.array_tool else {
                return true;
            };
            match row {
                "shape" => t.shape = from_label(&SHAPES, &text).unwrap_or(t.shape),
                "closed" => t.closed = text == "true" || text == "1",
                "mode" => t.mode = from_label(&MODES, &text).unwrap_or(t.mode),
                "value" => t.value = text.trim().parse().unwrap_or(t.value),
                "counts" => t.counts = parse_counts(&text),
                "corners" => t.corners = from_label(&CORNERS, &text).unwrap_or(t.corners),
                "orientation" => {
                    t.orientation = from_label(&ORIENTATIONS, &text).unwrap_or(t.orientation)
                }
                _ => {}
            }
            self.properties_stale = true;
            return true;
        }
        let Some(row) = property.strip_prefix("pa_") else {
            return false;
        };
        let frame = self.frame;
        let Some(id) = self.selection().first().copied() else {
            return true;
        };
        let Some(r) = self
            .store_now()
            .and_then(|s| s.get(id).and_then(path_arrays::path_array_recipe))
        else {
            return true;
        };
        let mut a = json!({ "array_id": id });
        let mut d = r.distribution.clone();
        let parse_range = |t: &str, angle: bool| -> Option<[f64; 2]> {
            let nums: Vec<f64> = t
                .replace("to", " ")
                .split([' ', ',', '–', '-'])
                .filter_map(|x| x.trim().trim_end_matches('°').parse().ok())
                .collect();
            match nums.as_slice() {
                [lo, hi] => Some(if angle {
                    [lo.to_radians(), hi.to_radians()]
                } else {
                    [*lo, *hi]
                }),
                _ => None,
            }
        };
        match row {
            "mode" => {
                d.mode = from_label(&MODES, &text).unwrap_or(d.mode);
                a["distribution"] = json!(d);
            }
            "value" => {
                d.value = text.trim().parse().unwrap_or(d.value);
                a["distribution"] = json!(d);
            }
            "counts" => {
                d.counts = parse_counts(&text);
                a["distribution"] = json!(d);
            }
            "corners" => a["corners"] = json!(from_label(&CORNERS, &text).unwrap_or(r.corners)),
            "orientation" => {
                a["orientation"] = json!(from_label(&ORIENTATIONS, &text).unwrap_or(r.orientation))
            }
            "include_start" => a["include_start"] = json!(text == "true" || text == "1"),
            "include_end" => a["include_end"] = json!(text == "true" || text == "1"),
            "offset" => a["offset"] = json!(text),
            "rotation_offset" => {
                a["rotation_offset"] = json!(if text.trim().ends_with('°') {
                    text.clone()
                } else {
                    format!("{}°", text.trim())
                })
            }
            "var_rotation" | "var_scale" => {
                let mut v = r.variation.clone();
                let range = parse_range(&text, row == "var_rotation");
                if row == "var_rotation" {
                    v.rotation = range
                } else {
                    v.scale = range
                }
                a["variation"] = json!(v);
            }
            "reseed" => a["reseed"] = json!(true),
            "excluded" => {
                if text.trim() == "0" {
                    let _ = self.run(
                        "array.restore_member",
                        json!({ "array_id": id, "index": "all" }),
                    );
                }
                self.properties_stale = true;
                self.refresh_scene(&frame);
                return true;
            }
            _ => return true,
        }
        if let Err(e) = self.run("array.set", a) {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// Modify › Unarray: the selected arrays' members become independent objects.
    pub(crate) fn unarray_selection(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let arrays: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| {
                store
                    .get(*i)
                    .and_then(path_arrays::path_array_recipe)
                    .is_some()
            })
            .collect();
        if arrays.is_empty() {
            return frame.set_status_text("Unarray: select a path array first", 0);
        }
        let mut made = 0;
        for a in arrays {
            match self.run("array.unarray", json!({ "array_id": a })) {
                Ok(v) => made += v.as_array().map_or(0, Vec::len),
                Err(e) => return error(&frame, &e),
            }
        }
        self.clear_selection();
        self.refresh_scene(&frame);
        frame.set_status_text(&format!("Unarrayed: {made} independent objects"), 0);
    }
}

fn distribution_rows(
    grid: &PropertyGrid,
    p: &str,
    mode: Mode,
    value: f64,
    counts: &[u32],
    corners: Corners,
    orientation: Orientation,
) {
    grid.append(Property::enumeration(
        "Distribution",
        format!("{p}_mode"),
        choices(&MODES),
        label(&MODES, mode),
    ));
    let (label_v, help) = match mode {
        Mode::Number => ("Number", "How many copies (per side with corners)"),
        _ => ("Spacing", "Metres"),
    };
    grid.append(Property::string(
        label_v,
        format!("{p}_value"),
        format!("{value}"),
    ));
    grid.set_help_string(format!("{p}_value"), help);
    let counts = counts
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    grid.append(Property::string(
        "Counts per Side",
        format!("{p}_counts"),
        counts,
    ));
    grid.set_help_string(
        format!("{p}_counts"),
        "With corners: a number for each side in turn, for example 8, 17",
    );
    grid.append(Property::enumeration(
        "Corners",
        format!("{p}_corners"),
        choices(&CORNERS),
        label(&CORNERS, corners),
    ));
    grid.append(Property::enumeration(
        "Orientation",
        format!("{p}_orientation"),
        choices(&ORIENTATIONS),
        label(&ORIENTATIONS, orientation),
    ));
}
