// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Doors and windows in the window (ARC-DOOR-001, ARC-WIN-001): the Door and Window tools, door
//! swings in plans, dragging a selected door or window along its wall, and their Properties
//! (type selector and type values, sill height, flips).
//!
//! The tools work in plans and 3D views: hovering a Reviera wall previews the insert there,
//! centred on the cursor and kept inside the wall; the side of the wall the cursor is on sets the
//! facing (a door's swing side), and Spacebar flips a door's hand. A click places it, and the
//! tool repeats until Esc.

use reviera_core::ElementId;
use reviera_core::inserts::{self, Kind, Placed};
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.1, 0.45, 0.95, 0.45];
const PREVIEW_LINES: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
/// The cursor must be this close to a wall (pixels, beyond its faces) to place in it.
const NEAR_PX: f32 = 8.0;
const DUPLICATE: &str = "Duplicate Type…";

/// The Door or Window tool.
#[derive(Debug)]
pub struct InsertTool {
    pub kind: Kind,
    /// The insert under the cursor, as it would be placed.
    hover: Option<Placed>,
    flip_hand: bool,
}

impl Gui {
    fn store_now(&self) -> Option<reviera_core::Store> {
        Some(self.app.document(self.doc?)?.doc.store().clone())
    }

    /// The type the tool of `kind` places: the one last chosen, else the default.
    pub(crate) fn tool_insert_type(&self, kind: Kind) -> Option<ElementId> {
        let store = self.store_now()?;
        let of_kind: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| inserts::insert_type(e).and_then(|t| t.kind()) == Some(kind))
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        let default = match kind {
            Kind::Door => inserts::DEFAULT_DOOR_TYPE,
            Kind::Window => inserts::DEFAULT_WINDOW_TYPE,
        };
        self.insert_types
            .get(&kind.class())
            .copied()
            .filter(|t| of_kind.iter().any(|(i, _)| i == t))
            .or_else(|| {
                of_kind
                    .iter()
                    .find(|(_, n)| n == default)
                    .or(of_kind.first())
                    .map(|(i, _)| *i)
            })
    }

    /// Starts the Door or Window tool.
    pub(crate) fn start_insert_tool(&mut self, kind: Kind) {
        let frame = self.frame;
        if self.doc.is_none() {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        }
        self.end_tool(&frame);
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_insert_tool();
        self.end_modify();
        self.leave_walk();
        self.insert_tool = Some(InsertTool {
            kind,
            hover: None,
            flip_hand: false,
        });
        self.clear_selection();
        self.properties_stale = true;
        self.insert_prompt();
    }

    pub(crate) fn end_insert_tool(&mut self) -> bool {
        if self.insert_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay(&[]);
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        self.frame.set_status_text("Ready", 0);
        true
    }

    fn insert_prompt(&mut self) {
        let Some(t) = &self.insert_tool else { return };
        let text = match (t.kind, &t.hover) {
            (Kind::Door, None) => {
                "Door: hover a wall; the side of the cursor is the swing side".into()
            }
            (Kind::Window, None) => "Window: hover a wall".to_owned(),
            (kind, Some(p)) => {
                let along = p.recipe.along - p.ty.width / 2.0;
                let shown = self
                    .app
                    .execute(
                        "units.format",
                        json!({ "value": along, "data_type": "length" }),
                        &super::user(),
                    )
                    .ok()
                    .and_then(|o| o.result.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{along:.3} m"));
                let extra = if kind == Kind::Door {
                    " Spacebar flips the hand."
                } else {
                    ""
                };
                format!("Click to place, {shown} from the wall's start.{extra} Esc ends the tool.")
            }
        };
        self.frame.set_status_text(&text, 0);
    }

    /// The Reviera wall under pixel (`x`, `y`) and the plan point on it, with the side of the
    /// wall the cursor is on (`true` for the exterior).
    pub(crate) fn wall_under(&self, x: i32, y: i32) -> Option<(ElementId, [f64; 2], bool)> {
        let v = self.active_view()?;
        let store = self.app.document(v.doc)?.doc.store();
        let size = v.panel.get_client_size();
        let (o, d) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        // The point of the wall's location curve nearest `p`, its direction there, and
        // whether `p` is on its +normal (left) side.
        let nearest = |e: &reviera_core::Element, g: &reviera_core::walls::WallGeometry, p: [f64; 2]| {
            let path = reviera_core::walls::wall_recipe(e)
                .and_then(|r| reviera_core::walls::curved::path_of(&r));
            let (foot, u) = match path {
                Some(path) => path.at(path.project(p)),
                None => {
                    let u = g.direction();
                    let t = ((p[0] - g.start[0]) * u[0] + (p[1] - g.start[1]) * u[1])
                        .clamp(0.0, g.length());
                    ([g.start[0] + u[0] * t, g.start[1] + u[1] * t], u)
                }
            };
            let left = u[0] * (p[1] - foot[1]) - u[1] * (p[0] - foot[0]) >= 0.0;
            (foot, u, left)
        };
        if let Some(plan) = &v.plan {
            // In a plan: the nearest wall cut by the plan, within its faces plus a few pixels.
            let cut = plan.range.cut;
            let point = [f64::from(o.x), f64::from(o.y)];
            let mut best: Option<(f64, ElementId, bool)> = None;
            for e in store.iter() {
                if reviera_core::walls::wall_recipe(e).is_none() {
                    continue;
                }
                let Ok(g) = reviera_core::walls::regenerate(store, e) else {
                    continue;
                };
                if cut < g.base_z || cut > g.base_z + g.height {
                    continue;
                }
                let (foot, u, left) = nearest(e, &g, point);
                let seg = [foot, [foot[0] + u[0] * 1e-3, foot[1] + u[1] * 1e-3]];
                let px = self.pixel_distance(x, y, seg, cut as f32);
                let model = (point[0] - foot[0]).hypot(point[1] - foot[1]);
                // Pixels per metre here, to widen the wall by a few pixels.
                let per_px = if px > 1e-3 {
                    model / f64::from(px)
                } else {
                    0.0
                };
                let reach = g.width / 2.0 + f64::from(NEAR_PX) * per_px;
                if model <= reach && best.is_none_or(|b| model < b.0) {
                    best = Some((model, e.id(), left));
                }
            }
            let (_, id, left) = best?;
            return Some((id, point, left));
        }
        // In 3D: the wall face under the cursor.
        let (i, t) = self
            .scene()
            .pick_within(o, d, f32::NEG_INFINITY, f32::INFINITY)?;
        let id = self.scene().meshes[i].element?;
        let e = store.get(id)?;
        reviera_core::walls::wall_recipe(e)?;
        let g = reviera_core::walls::regenerate(store, e).ok()?;
        let hit = o + d * t;
        let point = [f64::from(hit.x), f64::from(hit.y)];
        Some((id, point, nearest(e, &g, point).2))
    }

    /// The insert the tool would place at pixel (`x`, `y`): centred on the cursor along the
    /// wall, fitted to it (slid or shrunk into the free gap, raised onto the wall's open face).
    fn insert_candidate(&self, x: i32, y: i32) -> Option<Placed> {
        let t = self.insert_tool.as_ref()?;
        let (host, point, exterior) = self.wall_under(x, y)?;
        let store = self.store_now()?;
        let mut p = inserts::preview(
            &store,
            t.kind,
            self.tool_insert_type(t.kind),
            host,
            0.0,
            t.flip_hand,
            !exterior,
        )?;
        p.recipe.along = p.along_of(point);
        if !inserts::fit(&store, &mut p) {
            return None;
        }
        // To the millimetre.
        p.recipe.along = (p.recipe.along * 1000.0).round() / 1000.0;
        p.recipe.width = p.recipe.width.map(|w| (w * 1000.0).floor() / 1000.0);
        p.recipe.height = p.recipe.height.map(|h| (h * 1000.0).floor() / 1000.0);
        if let Some(w) = p.recipe.width {
            p.ty.width = w;
        }
        if let Some(h) = p.recipe.height {
            p.ty.height = h;
        }
        Some(p)
    }

    pub(crate) fn insert_motion(&mut self, x: i32, y: i32) {
        let hover = self.insert_candidate(x, y);
        if let Some(t) = &mut self.insert_tool {
            t.hover = hover;
        }
        self.insert_preview();
        self.insert_prompt();
    }

    pub(crate) fn insert_click(&mut self, x: i32, y: i32) {
        let Some(p) = self.insert_candidate(x, y) else {
            return self
                .frame
                .set_status_text("Hover a Reviera wall to place it in", 0);
        };
        let command = match p.kind {
            Kind::Door => "door.place",
            Kind::Window => "window.place",
        };
        let args = json!({
            "type_id": self.tool_insert_type(p.kind),
            "host_wall_id": p.host,
            "point_along": p.recipe.along,
            "sill_height": p.recipe.sill,
            "width": p.recipe.width,
            "height": p.recipe.height,
            "flip_hand": p.recipe.flip_hand,
            "flip_facing": p.recipe.flip_facing,
        });
        match self.run(command, args) {
            Ok(_) => {
                let frame = self.frame;
                self.refresh_scene(&frame);
                self.insert_motion(x, y);
            }
            Err(e) => self.frame.set_status_text(&e, 0),
        }
    }

    /// Spacebar in the Door tool flips the hand; without a tool it flips the selected doors.
    pub(crate) fn insert_key(&mut self, key: i32) -> bool {
        if key != WXK_SPACE {
            return false;
        }
        if let Some(t) = &mut self.insert_tool {
            if t.kind != Kind::Door {
                return true;
            }
            t.flip_hand = !t.flip_hand;
            if let Some(h) = &mut t.hover {
                h.recipe.flip_hand = t.flip_hand;
            }
            self.insert_preview();
            return true;
        }
        let ids = self.selected_inserts();
        if ids.is_empty() {
            return false;
        }
        let frame = self.frame;
        if let Err(e) = self.run("insert.flip_hand", json!({ "element_ids": ids })) {
            frame.set_status_text(&e, 0);
        }
        self.refresh_scene(&frame);
        true
    }

    fn insert_preview(&mut self) {
        let Some(t) = &self.insert_tool else { return };
        let hover = t.hover.clone();
        self.show_insert_overlay(hover.as_ref());
    }

    /// Draws `p` (or nothing) as the transient overlay: its meshes, and a door's swing.
    fn show_insert_overlay(&mut self, p: Option<&Placed>) {
        let mut meshes = Vec::new();
        let mut lines = Lines {
            color: PREVIEW_LINES,
            width: 0.25,
            segments: Vec::new(),
        };
        if let Some(p) = p {
            for ((verts, indices), _) in p.meshes() {
                meshes.push(reviera_render::SceneMesh {
                    element: None,
                    class: p.kind.class().into(),
                    positions: verts
                        .iter()
                        .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                        .collect(),
                    indices,
                    color: PREVIEW,
                });
            }
            let z = self
                .active_view()
                .and_then(|v| v.plan.as_ref())
                .map_or(p.z0, |pl| pl.range.cut) as f32
                + 0.003;
            lines.segments.extend(
                p.swing()
                    .into_iter()
                    .map(|s| s.map(|[x, y]| [x as f32, y as f32, z])),
            );
        }
        if let Some(v) = self.viewport() {
            v.set_overlay(&meshes);
            v.set_overlay_lines(&[lines]);
        }
    }

    /// The selected doors and windows.
    pub(crate) fn selected_inserts(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|id| store.get(*id).and_then(inserts::kind_of).is_some())
            .collect()
    }

    /// Door swings of plan view `index`: the doors in walls based on the plan's level, in the
    /// doors' projection pen (selected ones highlighted).
    pub(crate) fn insert_symbol_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone().filter(|p| !p.ceiling) else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let selected: Vec<ElementId> = self.selection();
        let level = store.get(plan.level).map(|l| l.guid().to_string());
        let pens = reviera_core::standards::pen_set(&store);
        let styles = reviera_core::standards::object_styles(&store);
        let z = plan.range.cut as f32 + 0.001;
        let mut out: Vec<Lines> = Vec::new();
        for p in inserts::placed(&store) {
            let on_level = store
                .get(p.host)
                .and_then(reviera_core::walls::wall_recipe)
                .is_some_and(|r| Some(&r.base_level) == level.as_ref());
            if !on_level {
                continue;
            }
            let (_, width) = reviera_core::standards::element_widths(
                &store,
                &pens,
                &styles,
                Some(p.id),
                f64::from(plan.scale),
            );
            let color = if selected.contains(&p.id) {
                reviera_render::HIGHLIGHT
            } else {
                reviera_render::plan::PROJECTION_LINE
            };
            out.push(Lines {
                color,
                width: width as f32,
                segments: p
                    .swing()
                    .into_iter()
                    .map(|s| s.map(|[x, y]| [x as f32, y as f32, z]))
                    .collect(),
            });
        }
        out
    }

    /// The door or window under pixel (`x`, `y`) of the active plan: inside its opening (with a
    /// few pixels to spare) or on its swing.
    pub(crate) fn insert_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let plan = v.plan.as_ref()?;
        let store = self.app.document(v.doc)?.doc.store();
        let point = self.plan_xy(x, y)?;
        let cut = plan.range.cut;
        let z = cut as f32;
        for p in inserts::placed(store) {
            if cut < p.wall.base_z || cut > p.wall.base_z + p.wall.height {
                continue;
            }
            let (u, n) = (p.wall.direction(), p.wall.normal());
            let d = [point[0] - p.wall.start[0], point[1] - p.wall.start[1]];
            let (along, across) = (d[0] * u[0] + d[1] * u[1], d[0] * n[0] + d[1] * n[1]);
            let c = p.cut();
            // A few pixels in metres here, from the wall's location line.
            let seg = [p.wall.start, p.wall.end];
            let px = self.pixel_distance(x, y, seg, z);
            let per_px = if px > 1e-3 {
                across.abs() / f64::from(px)
            } else {
                0.0
            };
            let spare = 4.0 * per_px;
            let inside = along >= c.x[0] - spare
                && along <= c.x[1] + spare
                && across.abs() <= p.wall.width / 2.0 + spare;
            let on_swing = p
                .swing()
                .into_iter()
                .any(|s| self.pixel_distance(x, y, s, z) <= NEAR_PX);
            if inside || on_swing {
                return Some(p.id);
            }
        }
        None
    }

    // ---- Dragging a selected insert along its wall ----

    /// Whether a left press at (`x`, `y`) in a plan starts dragging a selected door or window.
    pub(crate) fn insert_drag_start(
        &mut self,
        x: i32,
        y: i32,
        panel: &Panel,
    ) -> Option<(ElementId, [f64; 2])> {
        if !self.in_plan() || self.insert_tool.is_some() || self.tool.is_some() {
            return None;
        }
        let hit = self.element_at(x, y, panel)?;
        if !self.selected_inserts().contains(&hit) {
            return None;
        }
        Some((hit, self.plan_xy(x, y)?))
    }

    /// The insert being dragged, moved by the cursor's travel along its wall.
    fn dragged(&self, id: ElementId, from: [f64; 2], x: i32, y: i32) -> Option<(Placed, [f64; 2])> {
        let store = self.store_now()?;
        let mut p = inserts::resolve(&store, store.get(id)?).ok()?;
        let to = self.plan_xy(x, y)?;
        let u = p.wall.direction();
        let shift = (to[0] - from[0]) * u[0] + (to[1] - from[1]) * u[1];
        let shift = (shift * 1000.0).round() / 1000.0;
        p.recipe.along += shift;
        Some((p, [u[0] * shift, u[1] * shift]))
    }

    pub(crate) fn insert_drag_motion(&mut self, id: ElementId, from: [f64; 2], x: i32, y: i32) {
        let moved = self.dragged(id, from, x, y).map(|(p, _)| p);
        self.show_insert_overlay(moved.as_ref());
    }

    /// Releases a drag: moves the insert (one undoable step) unless it was a click.
    pub(crate) fn insert_drag_end(
        &mut self,
        id: ElementId,
        from: [f64; 2],
        x: i32,
        y: i32,
        click: bool,
    ) {
        self.show_insert_overlay(None);
        if click {
            return;
        }
        let Some((_, v)) = self.dragged(id, from, x, y) else {
            return;
        };
        let frame = self.frame;
        if let Err(e) = self.run(
            "element.move",
            json!({ "element_ids": [id], "vector": [v[0], v[1], 0.0] }),
        ) {
            frame.set_status_text(&e, 0);
        }
        self.refresh_scene(&frame);
    }

    // ---- Properties ----

    /// The palette rows of a door or window type (selector and values) for `kind`, applying to
    /// the selected inserts or, with none, to the tool.
    pub(crate) fn insert_type_rows(
        &mut self,
        grid: &PropertyGrid,
        kind: Kind,
        current: Option<ElementId>,
    ) {
        let Some(store) = self.store_now() else {
            return;
        };
        let mut types: Vec<(ElementId, String, inserts::InsertType)> = store
            .iter()
            .filter_map(|e| {
                let t = inserts::insert_type(e)?;
                (t.kind() == Some(kind)).then(|| (e.id(), e.name.clone().unwrap_or_default(), t))
            })
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut names: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        if current.is_some() {
            names.push((DUPLICATE.to_owned(), names.len() as i32));
        }
        grid.append(Property::enumeration("Type", "it_type", names, index));
        let Some((id, name, t)) = current.and_then(|c| types.into_iter().find(|t| t.0 == c)) else {
            return;
        };
        let used = reviera_core::types::instances(&store, store.get(id).expect("listed")).len();
        grid.append(Property::category(
            format!(
                "Type Parameters ({used} {}s)",
                kind.class().trim_start_matches("Ifc").to_lowercase()
            ),
            "c_inserttype",
        ));
        grid.append(Property::string("Type Name", "it_name", name));
        let labels = [
            ("width", "Width"),
            ("height", "Height"),
            ("frame_width", "Frame Width"),
            ("panel_thickness", "Panel Thickness"),
            ("frame_depth", "Frame Depth"),
            ("default_sill", "Default Sill Height"),
        ];
        let value = |k: &str| match k {
            "width" => t.width,
            "height" => t.height,
            "frame_width" => t.frame_width,
            "panel_thickness" => t.panel_thickness,
            "frame_depth" => t.frame_depth,
            _ => t.default_sill,
        };
        for (key, label) in labels.iter().filter(|(k, _)| t.params().contains(k)) {
            let text = self.format_length(value(key));
            grid.append(Property::string(*label, format!("it:{key}"), text));
        }
        self.type_rows = Some(id);
    }

    /// Instance rows of a selected door or window: sill height and flips.
    pub(crate) fn insert_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(p) = store.get(id).and_then(|e| inserts::resolve(&store, e).ok()) else {
            return;
        };
        let kind = p.kind;
        grid.append(Property::category("Placement", "c_insert"));
        let sill = self.format_length(p.recipe.sill);
        let head = self.format_length(p.recipe.sill + p.ty.height);
        grid.append(Property::string("Sill Height", "in_sill", sill));
        grid.set_help_string(
            "in_sill",
            "Bottom above the wall's base level; moves it, keeps its size",
        );
        grid.append(Property::string("Head Height", "in_head", head));
        grid.set_property_read_only("in_head", true);
        let (start, end) = (
            self.format_length(p.recipe.along),
            self.format_length(p.span() - p.recipe.along),
        );
        grid.append(Property::string("From Wall Start", "in_along", start));
        grid.append(Property::string("From Wall End", "in_from_end", end));
        grid.set_help_string(
            "in_along",
            "Its centre's distance from the wall's start, along the wall",
        );
        grid.set_help_string(
            "in_from_end",
            "Its centre's distance from the wall's end, along the wall",
        );
        grid.append(Property::category("Dimensions", "c_insert_size"));
        let (w, h) = (
            self.format_length(p.ty.width),
            self.format_length(p.ty.height),
        );
        grid.append(Property::string("Width", "in_width", w));
        grid.append(Property::string("Height", "in_height", h));
        grid.append(Property::boolean(
            "Own Size",
            "in_own",
            p.recipe.width.is_some() || p.recipe.height.is_some(),
        ));
        grid.set_help_string(
            "in_width",
            "This one's own width (the type's until changed); it stays centred",
        );
        grid.set_help_string("in_own", "Clear to return to the type's width and height");
        if kind == Kind::Door {
            grid.append(Property::boolean(
                "Flip Hand",
                "in_hand",
                p.recipe.flip_hand,
            ));
        }
        grid.append(Property::boolean(
            "Flip Facing",
            "in_facing",
            p.recipe.flip_facing,
        ));
        let current = store
            .get(id)
            .and_then(|e| e.param("@type")?.as_str().map(str::to_owned))
            .and_then(|g| reviera_core::GlobalId::parse(&g))
            .and_then(|g| store.find_by_guid(&g));
        grid.append(Property::category("Type", "c_insert_type"));
        self.insert_type_rows(grid, kind, current);
    }

    /// The palette while the Door or Window tool runs with nothing selected.
    pub(crate) fn insert_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(kind) = self.insert_tool.as_ref().map(|t| t.kind) else {
            return false;
        };
        let label = match kind {
            Kind::Door => "Door tool",
            Kind::Window => "Window tool",
        };
        grid.append(Property::category(label, "c_inserttool"));
        let current = self.tool_insert_type(kind);
        self.insert_type_rows(grid, kind, current);
        true
    }

    /// An edit of a door or window row; `false` if `property` is not one.
    pub(crate) fn insert_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected = self.selected_inserts();
        let result: Option<Result<(), String>> = match property {
            "in_sill" => Some(
                self.run(
                    "insert.set_sill_height",
                    json!({ "element_ids": selected, "sill_height": text }),
                )
                .map(|_| ()),
            ),
            "in_along" | "in_from_end" => {
                let key = if property == "in_along" {
                    "along"
                } else {
                    "from_end"
                };
                Some(selected.iter().try_for_each(|id| {
                    self.run(
                        "insert.set_position",
                        json!({ "element_id": id, key: text }),
                    )
                    .map(|_| ())
                }))
            }
            "in_width" | "in_height" => {
                let key = if property == "in_width" {
                    "width"
                } else {
                    "height"
                };
                Some(
                    self.run(
                        "insert.set_size",
                        json!({ "element_ids": selected, key: text }),
                    )
                    .map(|_| ()),
                )
            }
            "in_own" if text != "true" && text != "1" => Some(
                self.run(
                    "insert.set_size",
                    json!({ "element_ids": selected, "width": null, "height": null }),
                )
                .map(|_| ()),
            ),
            "in_own" => None,
            "in_hand" => Some(
                self.run("insert.flip_hand", json!({ "element_ids": selected }))
                    .map(|_| ()),
            ),
            "in_facing" => Some(
                self.run("insert.flip_facing", json!({ "element_ids": selected }))
                    .map(|_| ()),
            ),
            "it_name" => self.type_rows.map(|t| {
                self.run("type.rename", json!({ "type_id": t, "name": text }))
                    .map(|_| ())
            }),
            "it_type" => self.choose_insert_type(&text, &selected),
            p if p.starts_with("it:") => {
                let param = &p[3..];
                self.type_rows.map(|t| {
                    self.run(
                        "type.set_param",
                        json!({ "type_id": t, "param": param, "value": text }),
                    )
                    .map(|_| ())
                })
            }
            _ => return false,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        self.insert_preview();
        true
    }

    fn choose_insert_type(
        &mut self,
        choice: &str,
        selected: &[ElementId],
    ) -> Option<Result<(), String>> {
        let frame = self.frame;
        let store = self.store_now()?;
        let kind = match selected.first() {
            Some(id) => store.get(*id).and_then(inserts::kind_of)?,
            None => self.insert_tool.as_ref()?.kind,
        };
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
                .find(|e| {
                    inserts::insert_type(e).and_then(|t| t.kind()) == Some(kind)
                        && e.name.as_deref() == Some(choice)
                })?
                .id()
        };
        if selected.is_empty() {
            self.insert_types.insert(kind.class(), chosen);
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
}
