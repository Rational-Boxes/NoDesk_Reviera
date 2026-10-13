// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! wxWidgets UI shell through wxDragon (spec Ch 1).
//!
//! The main window: File and View menus, a status bar, the Properties palette (a property grid)
//! and the 3D viewport (`wgpu`, through `reviera-render-wgpu`). Clicking selects an element and
//! shows its parameters; edits in the palette go through `param.set`. Every action goes through
//! `App::execute`, like Lua and MCP. The ribbon, browsers and view tabs come next.

mod native;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use reviera_app::{App, AppConfig, DocumentId};
use reviera_core::{Caller, ElementId};
use reviera_render::{Camera, RenderBackend, RenderScene};
use reviera_render_wgpu::WgpuViewport;
use serde_json::{Value, json};
use wxdragon::prelude::*;

const ID_OPEN: i32 = ID_HIGHEST + 1;
const ID_SAVE_FILE: i32 = ID_HIGHEST + 2;
const ID_FIT: i32 = ID_HIGHEST + 3;

/// A press that moves less than this many pixels is a click (select), not a drag.
const CLICK_SLOP: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Orbit,
    Pan,
}

/// The parameter behind one palette row.
struct Row {
    element: ElementId,
    reference: String,
    data_type: String,
}

struct Gui {
    app: App,
    doc: Option<DocumentId>,
    scene: RenderScene,
    viewport: Option<WgpuViewport>,
    /// The native surface under the viewport; declared after it so it is dropped after it.
    native: Option<native::NativeView>,
    viewport_error: Option<String>,
    camera: Camera,
    drag: Option<(Drag, i32, i32)>,
    press: Option<(i32, i32)>,
    rows: Vec<Row>,
    properties_stale: bool,
}

fn user() -> Caller {
    Caller::user()
}

impl Gui {
    fn run(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.app
            .execute(name, args, &user())
            .map(|o| o.result)
            .map_err(|e| e.to_string())
    }

    /// Draws the viewport, creating it on first use (the panel must be realised by then).
    fn draw(&mut self, panel: &Panel, frame: &Frame) {
        let size = panel.get_client_size();
        if self.viewport.is_none() && self.viewport_error.is_none() {
            let created = native::NativeView::new(panel.get_handle()).and_then(|view| {
                let scale = view.scale_factor();
                let (w, h) = physical(size, scale);
                let viewport =
                    unsafe { WgpuViewport::for_window(view.display, view.window, w, h) }?;
                Ok((view, viewport))
            });
            match created {
                Ok((view, mut v)) => {
                    v.set_scene(&self.scene);
                    frame.set_status_text(
                        &format!("Graphics: {} on {}", v.adapter(), surface_kind(&view)),
                        1,
                    );
                    self.viewport = Some(v);
                    self.native = Some(view);
                }
                Err(e) => {
                    frame.set_status_text(&format!("3D viewport unavailable: {e}"), 0);
                    self.viewport_error = Some(e);
                }
            }
        }
        if let (Some(v), Some(view)) = (&mut self.viewport, &self.native) {
            view.update();
            let (w, h) = physical(size, view.scale_factor());
            v.resize(w, h);
            if let Err(e) = v.render(&self.camera) {
                frame.set_status_text(&e, 0);
            }
        }
    }

    fn open(&mut self, path: &Path, frame: &Frame) {
        let mut opened = self
            .app
            .execute("file.open", json!({ "path": path }), &user());
        // Unsaved work from a session that ended unexpectedly: offer to restore it (DM-LIVE-004).
        if let Err(e) = &opened
            && e.code == "recovery.pending"
        {
            let key = e.details["store_key"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let message = format!(
                "{} has unsaved work from a session that ended unexpectedly.\n\nRestore it? (No discards it.)",
                path.display()
            );
            opened = if ask(frame, &message) {
                self.app
                    .execute("recovery.restore", json!({ "store_key": key }), &user())
                    .map(|mut o| {
                        o.result = o.result["model_id"].clone();
                        o
                    })
            } else {
                let _ = self
                    .app
                    .execute("recovery.discard", json!({ "store_key": key }), &user());
                self.app
                    .execute("file.open", json!({ "path": path }), &user())
            };
        }
        if opened.as_ref().is_err_and(|e| e.code == "file.locked") {
            opened = self.app.execute(
                "file.open",
                json!({ "path": path, "read_only": true }),
                &user(),
            );
        }
        let id = match opened {
            Ok(o) => o.result.as_u64().unwrap_or_default(),
            Err(e) => return error(frame, &format!("Could not open {}:\n{e}", path.display())),
        };
        self.doc = Some(id);
        self.scene = match self.app.render_scene(id) {
            Ok(s) => s,
            Err(e) => return error(frame, &format!("Could not display the model:\n{e}")),
        };
        if let Some(v) = &mut self.viewport {
            v.set_scene(&self.scene);
            v.set_highlight(&[]);
        }
        self.fit();
        let info = self.app.documents().into_iter().find(|d| d.id == id);
        let title = info.as_ref().map_or("Untitled".into(), |d| d.title.clone());
        let read_only = info.is_some_and(|d| d.read_only);
        frame.set_title(&format!(
            "{title}{} - NoDesk Reviera",
            if read_only { " (read-only)" } else { "" }
        ));
        let elements = self.app.document(id).map_or(0, |d| d.doc.store().len());
        frame.set_status_text(
            &format!(
                "{title}: {elements} elements, {} triangles",
                self.scene.triangle_count()
            ),
            0,
        );
        self.properties_stale = true;
    }

    fn save(&mut self, frame: &Frame) {
        match self.run("file.save", json!({})) {
            Ok(v) => frame.set_status_text(
                &format!("Saved {}", v["path"].as_str().unwrap_or_default()),
                0,
            ),
            Err(e) => error(frame, &format!("Could not save:\n{e}")),
        }
    }

    fn fit(&mut self) {
        if let Some(b) = self.scene.bounds() {
            self.camera.fit(b);
        }
    }

    fn selection(&mut self) -> Vec<ElementId> {
        self.run("selection.get", json!({}))
            .ok()
            .and_then(|v| serde_json::from_value(v["ids"].clone()).ok())
            .unwrap_or_default()
    }

    /// Selects what is under pixel (`x`, `y`), or clears the selection over empty space.
    fn pick(&mut self, x: i32, y: i32, panel: &Panel) {
        if self.doc.is_none() {
            return;
        }
        let size = panel.get_client_size();
        let (origin, dir) =
            self.camera
                .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let hit = self
            .scene
            .pick(origin, dir)
            .and_then(|(i, _)| self.scene.meshes[i].element);
        let result = match hit {
            Some(id) => self.run("selection.set", json!({ "ids": [id] })),
            None => self.run("selection.clear", json!({})),
        };
        if result.is_ok() {
            self.selection_changed();
        }
    }

    fn clear_selection(&mut self) {
        if self.doc.is_some() && self.run("selection.clear", json!({})).is_ok() {
            self.selection_changed();
        }
    }

    fn selection_changed(&mut self) {
        let ids = self.selection();
        if let Some(v) = &mut self.viewport {
            v.set_highlight(&ids);
        }
        self.properties_stale = true;
    }

    /// Fills the Properties palette for the current selection.
    fn show_properties(&mut self, grid: &PropertyGrid) {
        self.fill_properties(grid);
        // Room for labels such as "Thermal Transmittance (type)".
        grid.set_splitter_position(150, 0);
    }

    fn fill_properties(&mut self, grid: &PropertyGrid) {
        self.properties_stale = false;
        grid.clear();
        self.rows.clear();
        let Some(doc) = self.doc else { return };
        let ids = self.selection();
        if ids.len() != 1 {
            let label = if ids.is_empty() {
                "No selection".to_owned()
            } else {
                format!("{} elements selected", ids.len())
            };
            grid.append(Property::category(label, "c_sel"));
            if let Ok(info) = self.run("selection.get", json!({})) {
                for (i, (cat, n)) in info["by_category"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let name = format!("s{i}");
                    grid.append(Property::string(category_label(cat), &name, n.to_string()));
                    grid.set_property_read_only(&name, true);
                }
            }
            return;
        }
        let id = ids[0];
        let Some(e) = self
            .app
            .document(doc)
            .and_then(|d| d.doc.store().get(id).cloned())
        else {
            return;
        };
        let store = &self.app.document(doc).expect("open").doc;
        let category = reviera_core::categories::category_of(&e)
            .map_or("—".to_owned(), |c| c.name.to_owned());
        let type_name = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.store().find_by_guid(&g))
            .and_then(|t| store.store().get(t))
            .and_then(|t| t.name.clone())
            .unwrap_or_default();
        grid.append(Property::category("Element", "c_element"));
        for (i, (label, value)) in [
            ("Category", category),
            ("IFC class", e.class.clone()),
            ("Name", e.name.clone().unwrap_or_default()),
            ("Type", type_name),
            ("ID", id.to_string()),
            ("GlobalId", e.guid().to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("e{i}");
            grid.append(Property::string(label, &name, value));
            grid.set_property_read_only(&name, true);
        }

        let defs = self
            .run("param.list", json!({ "element_id": id }))
            .unwrap_or(json!([]));
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        for d in defs.as_array().into_iter().flatten() {
            let group = d["group"].as_str().unwrap_or("other").to_owned();
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, list)) => list.push(d.clone()),
                None => groups.push((group, vec![d.clone()])),
            }
        }
        for (gi, (group, list)) in groups.into_iter().enumerate() {
            let mut header_added = false;
            for d in list {
                let reference = d["ref"].as_str().unwrap_or_default().to_owned();
                // Type parameters of an element without a type, and the like, are not shown.
                let Ok(value) = self.run(
                    "param.get",
                    json!({ "element_id": id, "param_ref": reference }),
                ) else {
                    continue;
                };
                if !header_added {
                    grid.append(Property::category(category_label(&group), format!("g{gi}")));
                    header_added = true;
                }
                let data_type = d["data_type"].as_str().unwrap_or("text").to_owned();
                let mut label = d["name"].as_str().unwrap_or_default().to_owned();
                if d["binding"] == "type" {
                    label.push_str(" (type)");
                }
                let name = format!("p{}", self.rows.len());
                let property = match (&value["value"], data_type.as_str()) {
                    (Value::Bool(b), _) => Property::boolean(&label, &name, *b),
                    (Value::Null, "yes_no") => Property::boolean(&label, &name, false),
                    _ => Property::string(
                        &label,
                        &name,
                        value["display"].as_str().unwrap_or_default(),
                    ),
                };
                grid.append(property);
                if d["read_only"].as_bool() == Some(true)
                    || value["read_only"].as_bool() == Some(true)
                {
                    grid.set_property_read_only(&name, true);
                }
                if let Some(tip) = d["tooltip"].as_str() {
                    grid.set_help_string(&name, tip);
                }
                self.rows.push(Row {
                    element: id,
                    reference,
                    data_type,
                });
            }
        }
    }

    /// Applies an edit made in the palette.
    fn edit(&mut self, property: &str, text: Option<String>, frame: &Frame) {
        let Some(index) = property
            .strip_prefix('p')
            .and_then(|i| i.parse::<usize>().ok())
        else {
            return;
        };
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let text = text.unwrap_or_default();
        let value = match row.data_type.as_str() {
            _ if text.trim().is_empty() => Value::Null,
            "yes_no" => json!(matches!(text.to_lowercase().as_str(), "true" | "1" | "yes")),
            "integer" => text.trim().parse::<i64>().map_or(json!(text), |n| json!(n)),
            "number" => text.trim().parse::<f64>().map_or(json!(text), |n| json!(n)),
            _ => json!(text),
        };
        let args =
            json!({ "element_ids": [row.element], "param_ref": row.reference, "value": value });
        if let Err(e) = self.run("param.set", args) {
            error(frame, &format!("The value was not changed:\n{e}"));
        }
        self.properties_stale = true;
    }
}

/// The viewport's size in device pixels.
fn physical(size: Size, scale: f64) -> (u32, u32) {
    let px = |v: i32| ((v.max(1) as f64) * scale).round() as u32;
    (px(size.width), px(size.height))
}

fn surface_kind(view: &native::NativeView) -> &'static str {
    match view.window {
        raw_window_handle::RawWindowHandle::Xlib(_) => "X11",
        raw_window_handle::RawWindowHandle::Wayland(_) => "Wayland",
        raw_window_handle::RawWindowHandle::Win32(_) => "Windows",
        raw_window_handle::RawWindowHandle::AppKit(_) => "macOS",
        _ => "native",
    }
}

/// A display name for a category key or parameter group (`identity_data` → "Identity Data").
fn category_label(key: &str) -> String {
    if let Some(c) = reviera_core::categories::category(key) {
        return c.name.to_owned();
    }
    if key.contains(' ') || key.chars().any(|c| c.is_uppercase()) {
        return key.to_owned();
    }
    key.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs `f` on the UI state unless another handler is using it (modal dialogs run nested event
/// loops, so paint and idle events can arrive while an action is in progress).
fn with(gui: &Rc<RefCell<Gui>>, f: impl FnOnce(&mut Gui)) {
    if let Ok(mut g) = gui.try_borrow_mut() {
        f(&mut g);
    }
}

fn ask(frame: &Frame, message: &str) -> bool {
    let dialog = MessageDialog::builder(frame, message, "NoDesk Reviera")
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build();
    dialog.show_modal() == ID_YES
}

fn error(frame: &Frame, message: &str) {
    let dialog = MessageDialog::builder(frame, message, "NoDesk Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
        .build();
    dialog.show_modal();
}

fn position(data: &WindowEventData) -> Option<(i32, i32)> {
    let p = match data {
        WindowEventData::MouseButton(e) => e.event.get_position(),
        WindowEventData::MouseMotion(e) => e.event.get_position(),
        _ => None,
    }?;
    Some((p.x, p.y))
}

fn wheel(data: &WindowEventData) -> i32 {
    match data {
        WindowEventData::MouseButton(e) => e.event.get_wheel_rotation(),
        WindowEventData::MouseMotion(e) => e.event.get_wheel_rotation(),
        WindowEventData::General(e) => e.get_wheel_rotation(),
        _ => 0,
    }
}

/// Runs the main window until it is closed, optionally opening `file`.
pub fn run(file: Option<PathBuf>) -> Result<(), String> {
    wxdragon::main(move |_| {
        let frame = Frame::builder()
            .with_title("NoDesk Reviera")
            .with_size(Size::new(1400, 860))
            .build();

        let file_menu = Menu::builder()
            .append_item(ID_OPEN, "&Open...\tCtrl+O", "Open an IFC project")
            .append_item(ID_SAVE_FILE, "&Save\tCtrl+S", "Save the project")
            .append_separator()
            .append_item(ID_EXIT, "E&xit", "Close Reviera")
            .build();
        let view_menu = Menu::builder()
            .append_item(ID_FIT, "Zoom to &Fit\tF", "Show the whole model")
            .build();
        let menu_bar = MenuBar::builder()
            .append(file_menu, "&File")
            .append(view_menu, "&View")
            .build();
        frame.set_menu_bar(menu_bar);
        frame.create_status_bar(2, 0, -1, "status");
        frame.set_status_text("Open an IFC file to begin (File > Open)", 0);

        let splitter = SplitterWindow::builder(&frame).build();
        let grid = PropertyGrid::builder(&splitter)
            .with_style(PropertyGridStyle::Tooltips)
            .build();
        let panel = Panel::builder(&splitter).build();
        panel.set_background_style(BackgroundStyle::Paint);
        splitter.split_vertically(&grid, &panel, 340);
        splitter.set_minimum_pane_size(200);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&splitter, 1, SizerFlag::Expand, 0);
        frame.set_sizer(sizer, true);

        let gui = Rc::new(RefCell::new(Gui {
            app: App::new(AppConfig::default()),
            doc: None,
            scene: RenderScene::default(),
            viewport: None,
            native: None,
            viewport_error: None,
            camera: Camera::default(),
            drag: None,
            press: None,
            rows: Vec::new(),
            properties_stale: true,
        }));

        {
            let (gui, p, f) = (gui.clone(), panel, frame);
            panel.on_paint(move |_| {
                let _dc = PaintDC::new(&p);
                with(&gui, |g| g.draw(&p, &f));
            });
        }
        {
            let p = panel;
            panel.on_size(move |e| {
                p.refresh(false, None);
                e.skip(true);
            });
        }
        for (button, kind) in [(0, Drag::Orbit), (1, Drag::Pan), (2, Drag::Pan)] {
            let (gui, p) = (gui.clone(), panel);
            let handler = move |e: WindowEventData| {
                if let Some((x, y)) = position(&e) {
                    let Ok(mut g) = gui.try_borrow_mut() else {
                        return;
                    };
                    g.drag = Some((kind, x, y));
                    if button == 0 {
                        g.press = Some((x, y));
                    }
                }
                p.set_focus();
            };
            match button {
                0 => panel.on_mouse_left_down(handler),
                1 => panel.on_mouse_middle_down(handler),
                _ => panel.on_mouse_right_down(handler),
            };
        }
        for button in 0..3 {
            let (gui, p) = (gui.clone(), panel);
            let handler = move |e: WindowEventData| {
                let Ok(mut g) = gui.try_borrow_mut() else {
                    return;
                };
                g.drag = None;
                let press = g.press.take();
                if button == 0
                    && let (Some((x0, y0)), Some((x, y))) = (press, position(&e))
                    && (x - x0).abs() <= CLICK_SLOP
                    && (y - y0).abs() <= CLICK_SLOP
                {
                    g.pick(x, y, &p);
                    drop(g);
                    p.refresh(false, None);
                }
            };
            match button {
                0 => panel.on_mouse_left_up(handler),
                1 => panel.on_mouse_middle_up(handler),
                _ => panel.on_mouse_right_up(handler),
            };
        }
        {
            let (gui, p) = (gui.clone(), panel);
            panel.on_mouse_motion(move |e| {
                let Some((x, y)) = position(&e) else { return };
                let Ok(mut g) = gui.try_borrow_mut() else {
                    return;
                };
                let Some((kind, x0, y0)) = g.drag else { return };
                let (dx, dy) = ((x - x0) as f32, (y - y0) as f32);
                match kind {
                    Drag::Orbit => g.camera.orbit(dx, dy),
                    Drag::Pan => {
                        let h = p.get_client_size().height as f32;
                        g.camera.pan(dx, dy, h);
                    }
                }
                g.drag = Some((kind, x, y));
                drop(g);
                p.refresh(false, None);
            });
        }
        {
            let (gui, p) = (gui.clone(), panel);
            panel.on_mouse_wheel(move |e| {
                let steps = wheel(&e) as f32 / 120.0;
                with(&gui, |g| g.camera.zoom(steps));
                p.refresh(false, None);
            });
        }
        {
            let (gui, p) = (gui.clone(), panel);
            panel.on_key_down(move |e| {
                if let WindowEventData::Keyboard(k) = &e
                    && k.get_key_code() == Some(WXK_ESCAPE)
                {
                    with(&gui, Gui::clear_selection);
                    p.refresh(false, None);
                } else {
                    e.skip(true);
                }
            });
        }
        {
            let (gui, f, g) = (gui.clone(), frame, grid);
            grid.on_changed(move |e| {
                if let Some(name) = e.property_name() {
                    let text = g.get_value_as_string(&name);
                    with(&gui, |gui| gui.edit(&name, text, &f));
                }
            });
        }
        {
            // Refresh the palette outside its own event handlers.
            let (gui, g) = (gui.clone(), grid);
            frame.on_idle(move |e| {
                if let Ok(mut gui) = gui.try_borrow_mut()
                    && gui.properties_stale
                {
                    gui.show_properties(&g);
                }
                e.skip(true);
            });
        }
        {
            let (gui, f, p) = (gui.clone(), frame, panel);
            frame.on_menu(move |e| match e.get_id() {
                ID_OPEN => {
                    let dialog = FileDialog::builder(&f)
                        .with_message("Open IFC project")
                        .with_wildcard("IFC files (*.ifc)|*.ifc;*.IFC|All files (*.*)|*.*")
                        .build();
                    if dialog.show_modal() == ID_OK
                        && let Some(path) = dialog.get_path()
                    {
                        with(&gui, |g| g.open(Path::new(&path), &f));
                        p.refresh(false, None);
                    }
                }
                ID_SAVE_FILE => with(&gui, |g| g.save(&f)),
                ID_FIT => {
                    with(&gui, Gui::fit);
                    p.refresh(false, None);
                }
                ID_EXIT => f.close(false),
                _ => e.skip(true),
            });
        }

        frame.show(true);
        frame.centre();
        if let Some(path) = &file {
            gui.borrow_mut().open(path, &frame);
            panel.refresh(false, None);
        }
    })
    .map_err(|e| e.to_string())
}
