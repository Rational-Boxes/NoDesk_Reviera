// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! wxWidgets UI shell through wxDragon (spec Ch 1).
//!
//! First increment: the main window with a File and View menu, a status bar and the 3D viewport
//! (`wgpu`, through `reviera-render-wgpu`). Every action goes through `App::execute`, like Lua and
//! MCP. The ribbon, browsers, properties palette and view tabs come next.

mod native;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use reviera_app::{App, AppConfig, DocumentId};
use reviera_core::Caller;
use reviera_render::{Camera, RenderBackend, RenderScene};
use reviera_render_wgpu::WgpuViewport;
use serde_json::json;
use wxdragon::prelude::*;

const ID_OPEN: i32 = ID_HIGHEST + 1;
const ID_SAVE_FILE: i32 = ID_HIGHEST + 2;
const ID_FIT: i32 = ID_HIGHEST + 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Orbit,
    Pan,
}

struct Gui {
    app: App,
    doc: Option<DocumentId>,
    scene: RenderScene,
    viewport: Option<WgpuViewport>,
    viewport_error: Option<String>,
    camera: Camera,
    drag: Option<(Drag, i32, i32)>,
}

impl Gui {
    /// Draws the viewport, creating it on first use (the panel must be realised by then).
    fn draw(&mut self, panel: &Panel, frame: &Frame) {
        let size = panel.get_client_size();
        let (w, h) = (size.width.max(1) as u32, size.height.max(1) as u32);
        if self.viewport.is_none() && self.viewport_error.is_none() {
            let created =
                native::handles(panel.get_handle()).and_then(|(display, window)| unsafe {
                    WgpuViewport::for_window(display, window, w, h)
                });
            match created {
                Ok(mut v) => {
                    v.set_scene(&self.scene);
                    frame.set_status_text(&format!("Graphics: {}", v.adapter()), 1);
                    self.viewport = Some(v);
                }
                Err(e) => {
                    frame.set_status_text(&format!("3D viewport unavailable: {e}"), 0);
                    self.viewport_error = Some(e);
                }
            }
        }
        if let Some(v) = &mut self.viewport {
            v.resize(w, h);
            if let Err(e) = v.render(&self.camera) {
                frame.set_status_text(&e, 0);
            }
        }
    }

    fn open(&mut self, path: &Path, frame: &Frame, panel: &Panel) {
        let user = Caller::user();
        let mut opened = self
            .app
            .execute("file.open", json!({ "path": path }), &user);
        if let Err(e) = &opened
            && e.code == "file.locked"
        {
            opened = self.app.execute(
                "file.open",
                json!({ "path": path, "read_only": true }),
                &user,
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
        panel.refresh(false, None);
    }

    fn save(&mut self, frame: &Frame) {
        match self.app.execute("file.save", json!({}), &Caller::user()) {
            Ok(o) => frame.set_status_text(
                &format!("Saved {}", o.result["path"].as_str().unwrap_or_default()),
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
    native::prefer_x11();
    wxdragon::main(move |_| {
        let frame = Frame::builder()
            .with_title("NoDesk Reviera")
            .with_size(Size::new(1280, 800))
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

        let panel = Panel::builder(&frame).build();
        panel.set_background_style(BackgroundStyle::Paint);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add(&panel, 1, SizerFlag::Expand, 0);
        frame.set_sizer(sizer, true);

        let gui = Rc::new(RefCell::new(Gui {
            app: App::new(AppConfig::default()),
            doc: None,
            scene: RenderScene::default(),
            viewport: None,
            viewport_error: None,
            camera: Camera::default(),
            drag: None,
        }));

        {
            let (gui, p, f) = (gui.clone(), panel, frame);
            panel.on_paint(move |_| {
                let _dc = PaintDC::new(&p);
                gui.borrow_mut().draw(&p, &f);
            });
        }
        {
            let p = panel;
            panel.on_size(move |e| {
                p.refresh(false, None);
                e.skip(true);
            });
        }
        for (button_down, kind) in [(0, Drag::Orbit), (1, Drag::Pan), (2, Drag::Pan)] {
            let gui = gui.clone();
            let handler = move |e: WindowEventData| {
                if let Some((x, y)) = position(&e) {
                    gui.borrow_mut().drag = Some((kind, x, y));
                }
            };
            match button_down {
                0 => panel.on_mouse_left_down(handler),
                1 => panel.on_mouse_middle_down(handler),
                _ => panel.on_mouse_right_down(handler),
            };
        }
        for up in 0..3 {
            let gui = gui.clone();
            let handler = move |_: WindowEventData| gui.borrow_mut().drag = None;
            match up {
                0 => panel.on_mouse_left_up(handler),
                1 => panel.on_mouse_middle_up(handler),
                _ => panel.on_mouse_right_up(handler),
            };
        }
        {
            let (gui, p) = (gui.clone(), panel);
            panel.on_mouse_motion(move |e| {
                let Some((x, y)) = position(&e) else { return };
                let mut g = gui.borrow_mut();
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
                gui.borrow_mut().camera.zoom(steps);
                p.refresh(false, None);
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
                        gui.borrow_mut().open(Path::new(&path), &f, &p);
                    }
                }
                ID_SAVE_FILE => gui.borrow_mut().save(&f),
                ID_FIT => {
                    gui.borrow_mut().fit();
                    p.refresh(false, None);
                }
                ID_EXIT => f.close(false),
                _ => e.skip(true),
            });
        }

        frame.show(true);
        frame.centre();
        if let Some(path) = &file {
            gui.borrow_mut().open(path, &frame, &panel);
        }
    })
    .map_err(|e| e.to_string())
}
