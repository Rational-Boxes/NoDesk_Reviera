// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! wxWidgets UI shell through wxDragon (spec Ch 1).
//!
//! The main window (UI-VIEWWIN-001/002): menus, a status bar, the Properties palette as a dockable
//! pane, and view windows as tabs in the centre (`wxAuiNotebook`), several models side by side.
//! Each view has its own camera and `wgpu` surface; all share one graphics device. Clicking selects
//! an element and shows its parameters; edits in the palette go through `param.set`. The Wall tool
//! draws chains of walls on a level with `wall.create`, and they show at once. Every action goes
//! through `App::execute`, like Lua and MCP. The ribbon and browsers come next.

mod browser;
mod loading;
mod native;
mod nav;
mod plans;
mod snap;
mod wx;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use reviera_app::{App, AppConfig, DocumentId};
use reviera_core::{Caller, ElementId};
use reviera_render::{Camera, RenderBackend, RenderScene};
use reviera_render_wgpu::{Gpu, WgpuViewport};
use serde_json::{Value, json};
use wxdragon::prelude::*;

const ID_OPEN: i32 = ID_HIGHEST + 1;
const ID_SAVE_FILE: i32 = ID_HIGHEST + 2;
const ID_FIT: i32 = ID_HIGHEST + 3;
const ID_NEW: i32 = ID_HIGHEST + 4;
const ID_UNDO: i32 = ID_HIGHEST + 5;
const ID_REDO: i32 = ID_HIGHEST + 6;
const ID_WALL: i32 = ID_HIGHEST + 7;
const ID_SAVE_AS: i32 = ID_HIGHEST + 8;
const ID_NEW_VIEW: i32 = ID_HIGHEST + 9;
const ID_CLOSE_VIEW: i32 = ID_HIGHEST + 10;
const ID_NEXT_VIEW: i32 = ID_HIGHEST + 11;
const ID_PREV_VIEW: i32 = ID_HIGHEST + 12;
const ID_TILE: i32 = ID_HIGHEST + 13;
const ID_TAB_ALL: i32 = ID_HIGHEST + 14;
const ID_SHOW_PROPERTIES: i32 = ID_HIGHEST + 15;
const ID_HOME: i32 = ID_HIGHEST + 16;
const ID_SET_HOME: i32 = ID_HIGHEST + 17;
const ID_PERSPECTIVE: i32 = ID_HIGHEST + 18;
const ID_ZOOM_PREV: i32 = ID_HIGHEST + 19;
const ID_ZOOM_NEXT: i32 = ID_HIGHEST + 20;
const ID_WALK: i32 = ID_HIGHEST + 21;
const ID_DEFAULT_3D: i32 = ID_HIGHEST + 22;
const ID_SHOW_BROWSER: i32 = ID_HIGHEST + 23;
const ID_BROWSER_OPEN: i32 = ID_HIGHEST + 24;
const ID_BROWSER_RENAME: i32 = ID_HIGHEST + 25;
const ID_BROWSER_DUPLICATE: i32 = ID_HIGHEST + 26;
const ID_BROWSER_DELETE: i32 = ID_HIGHEST + 27;
const ID_NEW_PLAN: i32 = ID_HIGHEST + 28;
/// The orientation presets of `nav::PRESETS`, one id each.
const ID_ORIENT_FIRST: i32 = ID_HIGHEST + 40;

/// Height of walls drawn on the top level (they reach the level above otherwise).
const WALL_HEIGHT: f64 = 3.0;

/// The colour of a wall being drawn (linear RGBA).
const PREVIEW: [f32; 4] = [0.1, 0.45, 0.95, 0.45];
/// The colour of snap markers and the exterior-side arrow.
const SNAP: [f32; 4] = [0.95, 0.55, 0.05, 1.0];

/// A press that moves less than this many pixels is a click (select), not a drag.
const CLICK_SLOP: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Orbit about a pivot (the selection's centre, or the view's target).
    Orbit(glam::Vec3),
    Pan,
    /// Turning the Orientation Cube (or, on its ring, turning about the vertical only).
    Cube {
        ring: bool,
    },
    /// Looking around in walk mode.
    Look,
}

/// The Wall tool: clicks on the level's plane add walls end to end.
struct WallTool {
    level: ElementId,
    level_name: String,
    elevation: f64,
    /// The level above, which new walls reach.
    top: Option<ElementId>,
    /// The end of the last wall, where the next one starts.
    last: Option<[f64; 2]>,
    /// The (snapped) point under the cursor.
    cursor: Option<snap::Snap>,
    /// Exterior on the right of the drawing direction (Spacebar).
    flip: bool,
    /// A length being typed, committed with Enter.
    typed: String,
    /// Location lines of the walls on the level, for snapping.
    lines: Vec<([f64; 2], [f64; 2])>,
}

/// The parameter behind one palette row.
struct Row {
    element: ElementId,
    reference: String,
    data_type: String,
}

/// A view window: one tab showing a 3D view of a model.
struct View {
    id: u32,
    doc: DocumentId,
    /// The model view shown; `None` for the unsaved `{3D}` view of a model that has no views.
    element: Option<ElementId>,
    name: String,
    /// The camera last exchanged with the session's view state.
    synced: Option<reviera_core::views::ViewCamera>,
    /// Set for plan views.
    plan: Option<plans::PlanState>,
    panel: Panel,
    viewport: Option<WgpuViewport>,
    /// The native surface under the viewport; declared after it so it is dropped after it.
    native: Option<native::NativeView>,
    error: Option<String>,
    camera: Camera,
    drag: Option<(Drag, i32, i32)>,
    press: Option<(i32, i32)>,
    nav: nav::Nav,
}

struct Gui {
    app: App,
    /// The shared handle of this state, for handlers of new views.
    this: Weak<RefCell<Gui>>,
    frame: Frame,
    notebook: AuiNotebook,
    aui: AuiManager,
    /// The document of the active view.
    doc: Option<DocumentId>,
    views: Vec<View>,
    active: Option<u32>,
    next_view: u32,
    gpu: Option<Gpu>,
    /// The scene of each open document, shared by its views.
    scenes: HashMap<DocumentId, RenderScene>,
    rows: Vec<Row>,
    properties_stale: bool,
    tool: Option<WallTool>,
    /// Drives animated view changes and walking.
    timer: Option<Timer<Frame>>,
    /// Models being opened in the background, and the timer that animates their tabs.
    loading: Vec<loading::Loading>,
    load_timer: Option<Timer<Frame>>,
    /// The Project Browser.
    browser: TreeCtrl,
    browser_items: Vec<(u64, ElementId)>,
    browser_stale: bool,
    /// The view a Project Browser context menu acts on.
    browser_target: Option<ElementId>,
    /// Feature edges of each model's scene, for plan graphics (computed when a plan needs them).
    plan_edges: HashMap<DocumentId, Rc<Vec<[[f32; 3]; 2]>>>,
    /// Letters typed toward a keyboard sequence such as `ZF` (§1.26.4), and when.
    keys: (String, std::time::Instant),
}

/// An empty scene and a default camera, for when no view is open.
static EMPTY: std::sync::LazyLock<RenderScene> = std::sync::LazyLock::new(RenderScene::default);
static DEFAULT_CAMERA: std::sync::LazyLock<Camera> = std::sync::LazyLock::new(Camera::default);

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

    fn view(&self, id: u32) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    fn active_view(&self) -> Option<&View> {
        self.active.and_then(|id| self.view(id))
    }

    fn active_view_mut(&mut self) -> Option<&mut View> {
        self.active
            .and_then(|id| self.views.iter_mut().find(|v| v.id == id))
    }

    /// The camera of the active view.
    fn camera(&self) -> &Camera {
        self.active_view().map_or(&DEFAULT_CAMERA, |v| &v.camera)
    }

    /// The viewport of the active view.
    fn viewport(&mut self) -> Option<&mut WgpuViewport> {
        self.active_view_mut()?.viewport.as_mut()
    }

    /// The scene of the active model.
    fn scene(&self) -> &RenderScene {
        self.doc.and_then(|d| self.scenes.get(&d)).unwrap_or(&EMPTY)
    }

    /// Draws a view, creating its surface on first use (the panel must be realised by then).
    fn draw(&mut self, id: u32) {
        let frame = self.frame;
        let Some(index) = self.views.iter().position(|v| v.id == id) else {
            return;
        };
        let doc = self.views[index].doc;
        let needs_surface =
            self.views[index].viewport.is_none() && self.views[index].error.is_none();
        let (scene, highlight) = if needs_surface {
            let highlight = if Some(doc) == self.doc {
                self.selection()
            } else {
                Vec::new()
            };
            (
                self.scenes.get(&doc).cloned().unwrap_or_default(),
                highlight,
            )
        } else {
            (RenderScene::default(), Vec::new())
        };
        let gpu = &mut self.gpu;
        let view = &mut self.views[index];
        let size = view.panel.get_client_size();
        if needs_surface {
            let created = native::NativeView::new(view.panel.get_handle()).and_then(|native| {
                let (w, h) = physical(size, native.scale_factor());
                let viewport = unsafe {
                    WgpuViewport::for_window_shared(gpu, native.display, native.window, w, h)
                }?;
                Ok((native, viewport))
            });
            match created {
                Ok((native, mut v)) => {
                    v.set_scene(&scene);
                    v.set_highlight(&highlight);
                    frame.set_status_text(
                        &format!("Graphics: {} on {}", v.adapter(), surface_kind(&native)),
                        1,
                    );
                    view.viewport = Some(v);
                    view.native = Some(native);
                }
                Err(e) => {
                    frame.set_status_text(&format!("3D viewport unavailable: {e}"), 0);
                    view.error = Some(e);
                }
            }
        }
        let Some(scale) = view.native.as_ref().map(native::NativeView::scale_factor) else {
            return;
        };
        if let (Some(v), Some(native)) = (&mut view.viewport, &view.native) {
            native.update();
            let (w, h) = physical(size, scale);
            v.resize(w, h);
        }
        self.update_plan_graphics(index);
        self.update_gizmo(index, scale);
        let view = &mut self.views[index];
        if let Some(v) = &mut view.viewport
            && let Err(e) = v.render(&view.camera)
        {
            frame.set_status_text(&e, 0);
        }
        // Navigation is view state of the session, saved with the next save (UI-NAV-001).
        let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
        let stored = view.camera.to_view(aspect);
        if let Some(element) = view.element
            && view.synced != Some(stored)
        {
            view.synced = Some(stored);
            let doc = view.doc;
            self.app.set_view_camera(doc, element, stored);
        }
    }

    /// A keyboard sequence letter (§1.26.4); returns whether it completed a known sequence.
    fn sequence_key(&mut self, c: char) -> bool {
        if self.keys.1.elapsed() > std::time::Duration::from_millis(1500) {
            self.keys.0.clear();
        }
        self.keys.0.push(c.to_ascii_uppercase());
        self.keys.1 = std::time::Instant::now();
        let frame = self.frame;
        let done = match self.keys.0.as_str() {
            "ZF" | "ZA" | "ZE" => {
                self.zoom_to_fit();
                true
            }
            "ZP" => {
                self.history(true);
                true
            }
            "ZN" => {
                self.history(false);
                true
            }
            "WA" => {
                self.start_wall_tool(&frame);
                true
            }
            "WK" => {
                self.toggle_walk();
                true
            }
            "PI" => {
                self.toggle_perspective();
                true
            }
            "Z" | "W" | "P" => false,
            _ => {
                self.keys.0.clear();
                false
            }
        };
        if done {
            self.keys.0.clear();
        }
        done
    }

    fn new_project(&mut self, frame: &Frame) {
        match self.run("file.new", json!({})) {
            Ok(id) => self.show_document(id.as_u64().unwrap_or_default(), frame),
            Err(e) => error(frame, &format!("Could not create a project:\n{e}")),
        }
    }

    /// Shows a newly opened model: its scene, and a 3D view tab.
    fn show_document(&mut self, id: DocumentId, frame: &Frame) {
        let scene = match self.app.render_scene(id) {
            Ok(s) => s,
            Err(e) => return error(frame, &format!("Could not display the model:\n{e}")),
        };
        let triangles = scene.triangle_count();
        self.scenes.insert(id, scene);
        let views = self
            .app
            .document(id)
            .map(|d| reviera_core::views::list(d.doc.store()))
            .unwrap_or_default();
        let first = views
            .iter()
            .find(|v| v.name == reviera_core::views::DEFAULT_3D)
            .or_else(|| {
                views
                    .iter()
                    .find(|v| v.kind == reviera_core::views::KIND_3D)
            })
            .map(|v| v.id);
        self.open_view(id, first);
        let elements = self.app.document(id).map_or(0, |d| d.doc.store().len());
        frame.set_status_text(
            &format!(
                "{}: {elements} elements, {triangles} triangles",
                self.doc_title(id)
            ),
            0,
        );
    }

    fn doc_info(&self, id: DocumentId) -> Option<reviera_app::DocumentInfo> {
        self.app.documents().into_iter().find(|d| d.id == id)
    }

    fn doc_title(&self, id: DocumentId) -> String {
        self.doc_info(id).map_or("Untitled".into(), |d| d.title)
    }

    /// The tab caption of a view: `{3D} - Model`, with `*` for unsaved changes.
    fn caption(&self, view: &View) -> String {
        let dirty = self.doc_info(view.doc).is_some_and(|d| d.dirty);
        format!(
            "{} - {}{}",
            self.view_name(view),
            self.doc_title(view.doc),
            if dirty { " *" } else { "" }
        )
    }

    /// A view's name: its model view's name, or `{3D}` for an unsaved one.
    fn view_name(&self, view: &View) -> String {
        view.element
            .and_then(|e| {
                self.app
                    .document(view.doc)?
                    .doc
                    .store()
                    .get(e)?
                    .name
                    .clone()
            })
            .unwrap_or_else(|| view.name.clone())
    }

    /// Opens `element` (a model view of `doc`) in a tab and activates it; an open view's tab is
    /// brought to the front. `None` opens an unsaved `{3D}` view.
    fn open_view(&mut self, doc: DocumentId, element: Option<ElementId>) {
        if let Some(e) = element
            && let Some(id) = self
                .views
                .iter()
                .find(|v| v.element == Some(e))
                .map(|v| v.id)
        {
            return self.activate_view(id);
        }
        let panel = Panel::builder(&self.notebook).build();
        panel.set_background_style(BackgroundStyle::Paint);
        let id = self.next_view;
        self.next_view += 1;
        // A window's shape is not known before it is shown; views keep their horizontal extent.
        let aspect = 1.6;
        let mut camera = Camera::default();
        match self.scenes.get(&doc).and_then(RenderScene::bounds) {
            Some(b) => camera.fit(b),
            // An empty project: look at the origin from above at an angle.
            None => camera.fit((
                glam::Vec3::new(-10.0, -10.0, 0.0),
                glam::Vec3::new(10.0, 10.0, 3.0),
            )),
        }
        let plan = self.plan_info(doc, element);
        if plan.is_some() {
            camera.perspective = false;
            camera.orient(glam::Vec3::Z);
        }
        let saved = element.and_then(|e| self.app.view_camera(doc, e));
        if let Some(c) = &saved {
            camera = Camera::from_view(c, aspect);
        }
        let mut nav = nav::Nav::default();
        nav.home = element
            .and_then(|e| self.app.view_home(doc, e))
            .map(|h| Camera::from_view(&h, aspect));
        let view = View {
            id,
            doc,
            element,
            name: reviera_core::views::DEFAULT_3D.into(),
            synced: saved,
            plan,
            panel,
            viewport: None,
            native: None,
            error: None,
            camera,
            drag: None,
            press: None,
            nav,
        };
        let caption = self.caption(&view);
        self.views.push(view);
        if let Some(gui) = self.this.upgrade() {
            attach_view_handlers(&gui, id, panel, self.frame);
        }
        self.notebook.add_page(&panel, &caption, true);
        self.activate_view(id);
        self.browser_stale = true;
    }

    /// Makes a view the active one; its model becomes the active document.
    fn activate_view(&mut self, id: u32) {
        let Some(doc) = self.view(id).map(|v| v.doc) else {
            return;
        };
        let frame = self.frame;
        if self.active != Some(id) {
            if self.doc != Some(doc) {
                self.end_tool(&frame);
            }
            if self.doc != Some(doc) {
                self.browser_stale = true;
            }
            self.active = Some(id);
            self.doc = Some(doc);
            let _ = self.app.activate(doc);
            let element = self.view(id).and_then(|v| v.element);
            self.app.set_active_view(doc, element);
            self.update_title(&frame);
            self.properties_stale = true;
        }
        if let Some(i) = self
            .view(id)
            .and_then(|v| wx::page_index(&self.notebook, &v.panel))
            && self.notebook.get_selection() != i as i32
        {
            self.notebook.set_selection(i);
        }
    }

    /// Updates the tab captions (titles and unsaved-changes marks).
    fn update_captions(&self) {
        for v in &self.views {
            if let Some(i) = wx::page_index(&self.notebook, &v.panel) {
                let caption = self.caption(v);
                if self.notebook.get_page_text(i) != caption {
                    self.notebook.set_page_text(i, &caption);
                }
            }
        }
    }

    /// Closes a view. Closing the last view of a model closes the model, asking to save unsaved
    /// changes first. Returns `false` when the user cancelled.
    fn close_view(&mut self, id: u32) -> bool {
        let frame = self.frame;
        let Some(doc) = self.view(id).map(|v| v.doc) else {
            return true;
        };
        if self.views.iter().filter(|v| v.doc == doc).count() == 1 {
            let info = self.doc_info(doc);
            let mut save = "no";
            if info.as_ref().is_some_and(|d| d.dirty) {
                let message = format!("Save changes to {} before closing?", self.doc_title(doc));
                let dialog = MessageDialog::builder(&frame, &message, "NoDesk Reviera")
                    .with_style(
                        MessageDialogStyle::YesNo
                            | MessageDialogStyle::Cancel
                            | MessageDialogStyle::IconQuestion,
                    )
                    .build();
                match dialog.show_modal() {
                    ID_YES => save = "yes",
                    ID_NO => {}
                    _ => return false,
                }
                if save == "yes" && info.as_ref().is_some_and(|d| d.path.is_none()) {
                    error(&frame, "Save the new project with File > Save As first.");
                    return false;
                }
            }
            if let Err(e) = self.run("file.close", json!({ "document_id": doc, "save": save })) {
                error(&frame, &format!("Could not close the model:\n{e}"));
                return false;
            }
            self.scenes.remove(&doc);
        }
        self.close_tab(id);
        true
    }

    /// Closes a view's tab (the model stays open).
    fn close_tab(&mut self, id: u32) {
        let frame = self.frame;
        if self.active == Some(id) {
            self.end_tool(&frame);
            self.active = None;
            self.doc = None;
        }
        let Some(index) = self.views.iter().position(|v| v.id == id) else {
            return;
        };
        // The surface goes before its window.
        let view = self.views.remove(index);
        let panel = view.panel;
        drop(view);
        if let Some(i) = wx::page_index(&self.notebook, &panel) {
            self.notebook.delete_page(i);
        }
        // The notebook selects a neighbour: make it the active view.
        let selected = usize::try_from(self.notebook.get_selection()).ok();
        match selected.and_then(|i| self.view_at(i)) {
            Some(v) => self.activate_view(v),
            None if self.views.is_empty() => {
                frame.set_title("NoDesk Reviera");
                self.properties_stale = true;
            }
            None => {}
        }
        self.browser_stale = true;
    }

    /// Closes every view; `false` when the user cancelled.
    fn close_all(&mut self) -> bool {
        while let Some(id) = self.views.last().map(|v| v.id) {
            if !self.close_view(id) {
                return false;
            }
        }
        true
    }

    /// The view on notebook page `index`.
    fn view_at(&self, index: usize) -> Option<u32> {
        self.views
            .iter()
            .find(|v| wx::page_index(&self.notebook, &v.panel) == Some(index))
            .map(|v| v.id)
    }

    /// Activates the next (`step` 1) or previous (-1) view in tab order.
    fn cycle_view(&mut self, step: i32) {
        let n = self.notebook.page_count() as i32;
        if n < 2 {
            return;
        }
        let current = self.notebook.get_selection().max(0);
        let next = (current + step).rem_euclid(n) as usize;
        if let Some(v) = self.view_at(next) {
            self.activate_view(v);
        }
    }

    /// Tile Views: every view visible at once, in columns, then rows (UI-VIEWWIN-002).
    fn tile(&mut self) {
        wx::unsplit_all(&self.notebook);
        let n = self.notebook.page_count();
        if n < 2 {
            return;
        }
        let columns = (n as f64).sqrt().ceil() as usize;
        // Splitting moves a page into a new tab group; going from the last page keeps the
        // indices of the pages still to move.
        for k in (1..n).rev() {
            let side = if k < columns {
                wx::Side::Right
            } else {
                wx::Side::Bottom
            };
            wx::split(&self.notebook, k, side);
        }
    }

    /// Tab Views: every view back in one tab group.
    fn tab_all(&mut self) {
        wx::unsplit_all(&self.notebook);
    }

    /// The active document's file, or `None` when it has never been saved.
    fn current_path(&self) -> Option<PathBuf> {
        let id = self.doc?;
        self.app.documents().into_iter().find(|d| d.id == id)?.path
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

    fn save_as(&mut self, path: &Path, frame: &Frame) {
        let mut path = path.to_owned();
        if path.extension().is_none() {
            path.set_extension("ifc");
        }
        match self.run("file.save_as", json!({ "path": path })) {
            Ok(_) => {
                self.update_title(frame);
                frame.set_status_text(&format!("Saved {}", path.display()), 0);
            }
            Err(e) => error(frame, &format!("Could not save:\n{e}")),
        }
    }

    fn update_title(&self, frame: &Frame) {
        let Some(id) = self.doc else { return };
        let info = self.app.documents().into_iter().find(|d| d.id == id);
        let title = info.as_ref().map_or("Untitled".into(), |d| d.title.clone());
        let read_only = info.is_some_and(|d| d.read_only);
        frame.set_title(&format!(
            "{title}{} - NoDesk Reviera",
            if read_only { " (read-only)" } else { "" }
        ));
    }

    /// Rebuilds the active model's scene after it changed (edits, undo, redo), in all its views.
    fn refresh_scene(&mut self, frame: &Frame) {
        let Some(id) = self.doc else { return };
        let scene = match self.app.render_scene(id) {
            Ok(scene) => scene,
            Err(e) => {
                return frame.set_status_text(&format!("Could not display the model: {e}"), 0);
            }
        };
        let ids = self.selection();
        for view in self.views.iter_mut().filter(|v| v.doc == id) {
            if let Some(v) = &mut view.viewport {
                v.set_scene(&scene);
                v.set_highlight(&ids);
            }
            view.panel.refresh(false, None);
        }
        self.scenes.insert(id, scene);
        self.plans_stale(id);
        self.refresh_plan_states();
        self.update_captions();
        self.properties_stale = true;
        self.browser_stale = true;
        self.sync_views();
    }

    fn undo_redo(&mut self, command: &str, frame: &Frame) {
        if self.doc.is_none() {
            return;
        }
        match self.run(command, json!({})) {
            Ok(_) => self.refresh_scene(frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Starts the Wall tool on the lowest level.
    fn start_wall_tool(&mut self, frame: &Frame) {
        self.leave_walk();
        let Some(doc) = self.doc.and_then(|id| self.app.document(id)) else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut levels: Vec<(f64, ElementId, String)> = doc
            .doc
            .store()
            .iter()
            .filter(|e| e.class == "IfcBuildingStorey")
            .map(|e| {
                let z = e.param("@Elevation").and_then(Value::as_f64).unwrap_or(0.0);
                let name = e
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Level {}", e.id()));
                (z, e.id(), name)
            })
            .collect();
        levels.sort_by(|a, b| a.0.total_cmp(&b.0));
        let in_plan = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| (p.elevation, p.level, p.level_name.clone()));
        let chosen = in_plan.or_else(|| levels.first().cloned());
        let Some((elevation, level, level_name)) = chosen else {
            return frame.set_status_text("The model has no levels to draw walls on", 0);
        };
        let top = levels.iter().find(|l| l.0 > elevation + 1e-6).map(|l| l.1);
        self.tool = Some(WallTool {
            level,
            level_name,
            elevation,
            top,
            last: None,
            cursor: None,
            flip: false,
            typed: String::new(),
            lines: Vec::new(),
        });
        self.update_wall_lines();
        self.clear_selection();
        self.tool_prompt(frame);
    }

    /// Caches the location lines of the walls on the tool's level, for snapping.
    fn update_wall_lines(&mut self) {
        let Some(t) = &self.tool else { return };
        let Some(doc) = self.doc.and_then(|id| self.app.document(id)) else {
            return;
        };
        let store = doc.doc.store();
        let level_guid = store.get(t.level).map(|e| e.guid().as_str().to_owned());
        let lines = store
            .iter()
            .filter_map(reviera_core::walls::wall_recipe)
            .filter(|r| Some(&r.base_level) == level_guid.as_ref())
            .map(|r| (r.curve.start, r.curve.end))
            .collect();
        self.tool.as_mut().expect("tool").lines = lines;
    }

    fn tool_prompt(&mut self, frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let exterior = if t.flip { "right" } else { "left" };
        let text = if !t.typed.is_empty() {
            format!("Wall length: {}_  (Enter to place, Esc to cancel)", t.typed)
        } else if t.last.is_none() {
            format!(
                "Wall on {}: click the start point. Exterior on the {exterior} (Spacebar flips).",
                t.level_name
            )
        } else {
            let mut text = String::from("Click the next point or type a length");
            if let (Some(start), Some(c)) = (t.last, t.cursor) {
                let length = (c.point[0] - start[0]).hypot(c.point[1] - start[1]);
                let shown = self
                    .app
                    .execute(
                        "units.format",
                        json!({"value": length, "data_type": "length"}),
                        &user(),
                    )
                    .ok()
                    .and_then(|o| o.result.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{length:.3} m"));
                text = format!("Length {shown}");
                if c.kind != snap::SnapKind::None {
                    text += &format!(" · {}", c.kind.label());
                }
            }
            format!("{text}. Exterior on the {exterior} (Spacebar flips). Esc ends the chain.")
        };
        frame.set_status_text(&text, 0);
    }

    fn end_tool(&mut self, frame: &Frame) {
        if self.tool.take().is_some() {
            if let Some(v) = self.viewport() {
                v.set_overlay(&[]);
            }
            frame.set_status_text("Ready", 0);
        }
    }

    /// Esc: clears a typed length, then ends the wall chain, then the tool; without a tool, clears
    /// the selection.
    fn escape(&mut self, frame: &Frame) {
        match &mut self.tool {
            Some(t) if !t.typed.is_empty() => {
                t.typed.clear();
                self.tool_prompt(frame);
            }
            Some(t) if t.last.is_some() => {
                t.last = None;
                self.update_overlay();
                self.tool_prompt(frame);
            }
            Some(_) => self.end_tool(frame),
            None => self.clear_selection(),
        }
    }

    /// The point on the tool's level plane under pixel (`x`, `y`).
    fn plane_point(&self, x: i32, y: i32, panel: &Panel) -> Option<[f64; 2]> {
        let t = self.tool.as_ref()?;
        let size = panel.get_client_size();
        let (o, d) = self
            .camera()
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        if d.z.abs() < 1e-6 {
            return None;
        }
        let s = (t.elevation as f32 - o.z) / d.z;
        (s > 0.0).then(|| {
            let p = o + d * s;
            [f64::from(p.x), f64::from(p.y)]
        })
    }

    /// The snapped point under pixel (`x`, `y`).
    fn snapped(&self, x: i32, y: i32, panel: &Panel) -> Option<snap::Snap> {
        let t = self.tool.as_ref()?;
        let raw = self.plane_point(x, y, panel)?;
        let size = panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let z = t.elevation as f32;
        let to_screen = |p: [f64; 2]| {
            self.camera()
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let mut s = snap::snap(raw, (x as f32, y as f32), t.last, &t.lines, to_screen);
        if s.kind == snap::SnapKind::None {
            // Free points to the millimetre.
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    /// Mouse movement in the Wall tool: snap, preview and prompt.
    fn wall_motion(&mut self, x: i32, y: i32, panel: &Panel, frame: &Frame) {
        let cursor = self.snapped(x, y, panel);
        let Some(t) = &mut self.tool else { return };
        t.cursor = cursor;
        self.update_overlay();
        self.tool_prompt(frame);
    }

    fn wall_click(&mut self, x: i32, y: i32, panel: &Panel, frame: &Frame) {
        let Some(p) = self.snapped(x, y, panel) else {
            return;
        };
        let Some(t) = &mut self.tool else { return };
        t.cursor = Some(p);
        if t.last.is_none() {
            t.last = Some(p.point);
            self.update_overlay();
            return self.tool_prompt(frame);
        }
        self.place_wall(p.point, frame);
    }

    /// Creates a wall from the chain's last point to `end`, and continues the chain from `end`.
    fn place_wall(&mut self, end: [f64; 2], frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let Some(start) = t.last else { return };
        let top = match t.top {
            Some(level) => json!({ "level": level }),
            None => json!({ "unconnected": WALL_HEIGHT }),
        };
        let args = json!({
            "curve": {"line": {"start": start, "end": end}},
            "base_level": t.level,
            "top_constraint": top,
            "flip": t.flip,
        });
        match self.run("wall.create", args) {
            Ok(_) => {
                let t = self.tool.as_mut().expect("tool");
                t.last = Some(end);
                t.typed.clear();
                self.update_wall_lines();
                self.refresh_scene(frame);
                self.update_overlay();
                self.tool_prompt(frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Keys in the Wall tool: Spacebar flips, digits and units type a length, Enter places it,
    /// Backspace edits it. Returns whether the key was used.
    fn wall_key(&mut self, key: i32, frame: &Frame) -> bool {
        let Some(t) = &mut self.tool else {
            // Without a tool, Spacebar flips the selected walls.
            if key == WXK_SPACE && self.doc.is_some() {
                let ids = self.selection();
                if !ids.is_empty() {
                    match self.run("wall.flip", json!({ "wall_ids": ids })) {
                        Ok(_) => frame.set_status_text("Flipped", 0),
                        Err(e) => frame.set_status_text(&e, 0),
                    }
                    self.refresh_scene(frame);
                    return true;
                }
            }
            return false;
        };
        match key {
            WXK_SPACE if t.typed.is_empty() => {
                t.flip = !t.flip;
                self.update_overlay();
            }
            WXK_BACK if !t.typed.is_empty() => {
                t.typed.pop();
            }
            WXK_RETURN | WXK_NUMPAD_ENTER if !t.typed.is_empty() => {
                self.commit_typed_length(frame);
                return true;
            }
            _ => {
                let Some(c) = u32::try_from(key).ok().and_then(char::from_u32) else {
                    return false;
                };
                // A length can start with a digit or a decimal point; after that, units and
                // separators too (`4.5 m`, `12' 6 1/2"`).
                let starts = c.is_ascii_digit() || c == '.';
                let continues = !t.typed.is_empty() && (c.is_ascii_graphic() || c == ' ');
                if t.last.is_none() || !(starts || continues) {
                    return false;
                }
                t.typed.push(c);
            }
        }
        self.tool_prompt(frame);
        true
    }

    /// Places a wall of the typed length toward the cursor.
    fn commit_typed_length(&mut self, frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let (Some(start), Some(cursor)) = (t.last, t.cursor) else {
            return;
        };
        let text = t.typed.clone();
        let length = match self.run("units.parse", json!({"text": text, "data_type": "length"})) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(e) => return frame.set_status_text(&format!("Invalid length: {e}"), 0),
        };
        let (dx, dy) = (cursor.point[0] - start[0], cursor.point[1] - start[1]);
        let d = dx.hypot(dy);
        if d < 1e-9 {
            return frame
                .set_status_text("Move the cursor to show the direction, then press Enter", 0);
        }
        let end = [start[0] + dx / d * length, start[1] + dy / d * length];
        self.place_wall(end.map(|v| (v * 1e6).round() / 1e6), frame);
    }

    /// The transient geometry of the Wall tool: the wall to be placed, its exterior side, and the
    /// snap marker.
    fn update_overlay(&mut self) {
        let Some(t) = &self.tool else { return };
        let mut overlay = Vec::new();
        let mesh = |(verts, indices): (Vec<[f64; 3]>, Vec<u32>), color| reviera_render::SceneMesh {
            element: None,
            class: "IfcWall".into(),
            positions: verts
                .iter()
                .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                .collect(),
            indices,
            color,
        };
        let marker = f64::from(self.camera().distance) * 0.008;
        if let Some(c) = t.cursor {
            let [x, y] = c.point;
            let size = if c.kind == snap::SnapKind::None {
                marker * 0.5
            } else {
                marker
            };
            let square = [
                [x - size, y - size],
                [x + size, y - size],
                [x + size, y + size],
                [x - size, y + size],
            ];
            let color = if c.kind == snap::SnapKind::None {
                PREVIEW
            } else {
                SNAP
            };
            overlay.push(mesh(
                reviera_core::walls::prism(&square, t.elevation, t.elevation + size),
                color,
            ));
        }
        if let (Some(start), Some(c)) = (t.last, t.cursor)
            && t.typed.is_empty()
        {
            let height = t
                .top
                .and_then(|l| {
                    self.doc
                        .and_then(|d| self.app.document(d))?
                        .doc
                        .store()
                        .get(l)?
                        .param("@Elevation")?
                        .as_f64()
                })
                .map_or(WALL_HEIGHT, |z| z - t.elevation);
            let g = reviera_core::walls::WallGeometry {
                start,
                end: c.point,
                base_z: t.elevation,
                height,
                width: 0.2,
            };
            if g.length() > reviera_core::walls::TOL_SHORT {
                overlay.push(mesh(g.mesh(), PREVIEW));
                // A flat arrow at mid-length on the exterior face.
                let side = if t.flip { -1.0 } else { 1.0 };
                let [nx, ny] = g.normal().map(|v| v * side);
                let [ux, uy] = g.direction();
                let mid = [(start[0] + c.point[0]) / 2.0, (start[1] + c.point[1]) / 2.0];
                let base = [mid[0] + nx * 0.1, mid[1] + ny * 0.1];
                let a = 0.25f64.min(g.length() / 4.0);
                let tip = [base[0] + nx * a, base[1] + ny * a];
                let z = t.elevation + 0.01;
                let positions = [
                    [base[0] - ux * a, base[1] - uy * a, z],
                    [base[0] + ux * a, base[1] + uy * a, z],
                    [tip[0], tip[1], z],
                ];
                overlay.push(mesh((positions.to_vec(), vec![0, 1, 2, 0, 2, 1]), SNAP));
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay(&overlay);
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
            self.camera()
                .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (bottom, top) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or((f32::NEG_INFINITY, f32::INFINITY), plans::PlanState::drawn);
        let hit = self
            .scene()
            .pick_within(origin, dir, bottom, top)
            .and_then(|(i, _)| self.scene().meshes[i].element);
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
        if let Some(v) = self.viewport() {
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
        if ids.is_empty() && self.view_properties(grid) {
            return;
        }
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

    /// The active view's properties; `false` when the active view is not a model view.
    fn view_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(v) = self.active_view() else {
            return false;
        };
        let Some(element) = v.element else {
            return false;
        };
        let name = self.view_name(v);
        let camera = v.camera;
        if let Some(plan) = v.plan.clone() {
            grid.append(Property::category("Floor Plan", "c_view"));
            grid.append(Property::string("View Name", "view_name", name));
            let fmt = |g: &mut Gui, z: f64| {
                if !z.is_finite() {
                    return "Unlimited".to_owned();
                }
                g.run(
                    "units.format",
                    json!({ "value": z - plan.elevation, "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:.3} m", z - plan.elevation))
            };
            let cut = fmt(self, plan.range.cut);
            grid.append(Property::string("Cut Plane", "view_cut", cut));
            grid.set_help_string(
                "view_cut",
                "Height of the cut plane above the associated level",
            );
            let rows = [
                ("Associated Level", plan.level_name.clone()),
                ("Top", fmt(self, plan.range.top)),
                ("Bottom", fmt(self, plan.range.bottom)),
                ("View Depth", fmt(self, plan.range.depth)),
                ("ID", element.to_string()),
            ];
            for (i, (label, value)) in rows.into_iter().enumerate() {
                let key = format!("v{i}");
                grid.append(Property::string(label, &key, value));
                grid.set_property_read_only(&key, true);
            }
            return true;
        }
        grid.append(Property::category("3D View", "c_view"));
        grid.append(Property::string("View Name", "view_name", name));
        let projection = if camera.perspective {
            "Perspective"
        } else {
            "Isometric (parallel)"
        };
        for (i, (label, value)) in [
            ("Projection", projection.to_owned()),
            (
                "Field of View",
                format!("{:.0}° vertical", camera.fov.to_degrees()),
            ),
            ("ID", element.to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("v{i}");
            grid.append(Property::string(label, &key, value));
            grid.set_property_read_only(&key, true);
        }
        true
    }

    /// Applies an edit made in the palette.
    fn edit(&mut self, property: &str, text: Option<String>, frame: &Frame) {
        if property == "view_cut" {
            if let Some(t) = text {
                self.set_cut_plane(&t, frame);
            }
            return;
        }
        if property == "view_name" {
            let element = self.active_view().and_then(|v| v.element);
            if let (Some(view), Some(name)) = (element, text) {
                if let Err(e) =
                    self.run("view.rename", json!({ "view_id": view, "new_name": name }))
                {
                    error(frame, &format!("The view was not renamed:\n{e}"));
                }
                self.after_view_change();
            }
            return;
        }
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
        // A level's elevation or a type's values can move walls.
        self.refresh_scene(frame);
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

/// Runs `f` on the view `id` made active first (input in a view applies to that view).
fn with_view(gui: &Rc<RefCell<Gui>>, id: u32, f: impl FnOnce(&mut Gui)) {
    with(gui, |g| {
        if g.view(id).is_some() {
            g.activate_view(id);
            f(g);
        }
    });
}

/// Mouse, keyboard and paint handlers of one view's panel.
fn attach_view_handlers(gui: &Rc<RefCell<Gui>>, id: u32, panel: Panel, frame: Frame) {
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_paint(move |_| {
            let _dc = PaintDC::new(&p);
            with(&gui, |g| g.draw(id));
        });
    }
    {
        let p = panel;
        panel.on_size(move |e| {
            p.refresh(false, None);
            e.skip(true);
        });
    }
    // Left: the Orientation Cube, else orbit (look in walk mode). Shift+middle: orbit.
    // Middle and right: pan. Right on the cube: its menu.
    for button in 0..3 {
        let (gui, p) = (gui.clone(), panel);
        let handler = move |e: WindowEventData| {
            let shift = match &e {
                WindowEventData::MouseButton(m) => m.event.event.shift_down(),
                _ => false,
            };
            let mut menu = None;
            if let Some((x, y)) = position(&e) {
                with_view(&gui, id, |g| {
                    let cube = g.cube_pick(x, y);
                    if button == 2 && cube.is_some() {
                        menu = Some(g.cube_menu());
                        return;
                    }
                    let walking = g.active_view().is_some_and(|v| v.nav.walk.is_some());
                    let plan = g.in_plan();
                    let kind = match (button, cube) {
                        _ if plan => Drag::Pan,
                        (0, Some(hit)) => {
                            if let Some(v) = g.active_view_mut() {
                                v.nav.cube_press = Some(hit);
                            }
                            Drag::Cube {
                                ring: hit == reviera_render::viewcube::CubeHit::Ring,
                            }
                        }
                        (0, None) if walking => Drag::Look,
                        (0, None) => Drag::Orbit(g.pivot()),
                        (1, _) if shift && !walking => Drag::Orbit(g.pivot()),
                        _ => Drag::Pan,
                    };
                    if !matches!(kind, Drag::Cube { .. }) {
                        g.remember();
                    }
                    if let Some(v) = g.active_view_mut() {
                        v.drag = Some((kind, x, y));
                        if button == 0 {
                            v.press = Some((x, y));
                        }
                    }
                });
            }
            p.set_focus();
            if let Some(mut menu) = menu {
                p.popup_menu(&mut menu, None);
            }
        };
        match button {
            0 => panel.on_mouse_left_down(handler),
            1 => panel.on_mouse_middle_down(handler),
            _ => panel.on_mouse_right_down(handler),
        };
    }
    for button in 0..3 {
        let (gui, p, f) = (gui.clone(), panel, frame);
        let handler = move |e: WindowEventData| {
            with_view(&gui, id, |g| {
                let Some(v) = g.active_view_mut() else { return };
                v.drag = None;
                let press = v.press.take();
                let cube = v.nav.cube_press.take();
                let Some((x, y)) = position(&e) else { return };
                let click = press.is_some_and(|(x0, y0)| {
                    (x - x0).abs() <= CLICK_SLOP && (y - y0).abs() <= CLICK_SLOP
                });
                if button != 0 || !click {
                    return;
                }
                match cube {
                    Some(hit) => g.cube_click(hit),
                    None if g.active_view().is_some_and(|v| v.nav.walk.is_some()) => {}
                    None if g.tool.is_some() => g.wall_click(x, y, &p, &f),
                    None => g.pick(x, y, &p),
                }
            });
            p.refresh(false, None);
        };
        match button {
            0 => panel.on_mouse_left_up(handler),
            1 => panel.on_mouse_middle_up(handler),
            _ => panel.on_mouse_right_up(handler),
        };
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_mouse_motion(move |e| {
            let Some((x, y)) = position(&e) else { return };
            let mut redraw = false;
            with(&gui, |g| {
                let Some(v) = g.views.iter().find(|v| v.id == id) else {
                    return;
                };
                let Some((kind, x0, y0)) = v.drag else {
                    if g.active != Some(id) {
                        return;
                    }
                    redraw |= g.cube_hover(Some((x, y)));
                    if g.tool.is_some() {
                        g.wall_motion(x, y, &p, &f);
                        redraw = true;
                    }
                    return;
                };
                let v = g.views.iter_mut().find(|v| v.id == id).expect("found");
                let (dx, dy) = ((x - x0) as f32, (y - y0) as f32);
                match kind {
                    Drag::Orbit(pivot) => v.camera.orbit_about(dx, dy, pivot),
                    Drag::Cube { ring: false } => v.camera.orbit(dx, dy),
                    Drag::Cube { ring: true } => v.camera.orbit(dx, 0.0),
                    Drag::Look => v.camera.look(dx, dy),
                    Drag::Pan => {
                        let h = p.get_client_size().height as f32;
                        v.camera.pan(dx, dy, h);
                    }
                }
                v.drag = Some((kind, x, y));
                redraw = true;
            });
            if redraw {
                p.refresh(false, None);
            }
        });
    }
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_mouse_leave(move |e| {
            let mut redraw = false;
            with(&gui, |g| {
                if g.active == Some(id) {
                    redraw = g.cube_hover(None);
                }
            });
            if redraw {
                p.refresh(false, None);
            }
            e.skip(true);
        });
    }
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_mouse_wheel(move |e| {
            let steps = wheel(&e) as f32 / 120.0;
            let at = position(&e);
            with_view(&gui, id, |g| {
                if g.walk_speed(steps) {
                    return;
                }
                g.remember_wheel();
                let size = p.get_client_size();
                let Some(v) = g.active_view_mut() else { return };
                match at {
                    Some((x, y)) => v.camera.zoom_at(
                        steps,
                        x as f32,
                        y as f32,
                        size.width as f32,
                        size.height as f32,
                    ),
                    None => v.camera.zoom(steps),
                }
            });
            p.refresh(false, None);
        });
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_key_down(move |e| {
            let (code, ctrl, shift) = match &e {
                WindowEventData::Keyboard(k) => {
                    (k.get_key_code(), k.control_down(), k.shift_down())
                }
                _ => (None, false, false),
            };
            let mut used = false;
            match code {
                // Ctrl+Tab / Ctrl+Shift+Tab switch views (GTK menus cannot bind them).
                Some(WXK_TAB) if ctrl => {
                    with(&gui, |g| g.cycle_view(if shift { -1 } else { 1 }));
                    used = true;
                }
                Some(WXK_ESCAPE) => {
                    with_view(&gui, id, |g| {
                        if g.active_view().is_some_and(|v| v.nav.walk.is_some()) {
                            g.toggle_walk();
                        } else {
                            g.escape(&f);
                        }
                    });
                    used = true;
                }
                Some(key) => {
                    with_view(&gui, id, |g| {
                        used = g.walk_key(key, true, shift);
                        if !used
                            && matches!(key, WXK_SPACE | WXK_BACK | WXK_RETURN | WXK_NUMPAD_ENTER)
                        {
                            used = g.wall_key(key, &f);
                        }
                    });
                }
                None => {}
            }
            if used {
                p.refresh(false, None);
            } else {
                // Lets the character event follow (typed lengths, key sequences).
                e.skip(true);
            }
        });
    }
    {
        let gui = gui.clone();
        panel.on_key_up(move |e| {
            if let WindowEventData::Keyboard(k) = &e
                && let Some(key) = k.get_key_code()
            {
                let shift = k.shift_down();
                with(&gui, |g| {
                    g.walk_key(key, false, shift);
                });
            }
            e.skip(true);
        });
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_char(move |e| {
            let key = match &e {
                WindowEventData::Keyboard(k) => k.get_unicode_key(),
                _ => None,
            };
            let mut used = false;
            if let Some(key) = key.filter(|k| *k > 32 && *k != 127) {
                with_view(&gui, id, |g| {
                    if g.active_view().is_some_and(|v| v.nav.walk.is_some()) {
                        // Movement keys are handled on key down; other letters may end the
                        // walk with a sequence (WK, ZF, ...).
                        if let Some(c) = u32::try_from(key).ok().and_then(char::from_u32)
                            && c.is_ascii_alphabetic()
                        {
                            g.sequence_key(c);
                        }
                        used = true;
                        return;
                    }
                    used = g.wall_key(key, &f);
                    let typing = g.tool.as_ref().is_some_and(|t| !t.typed.is_empty());
                    if !used
                        && !typing
                        && let Some(c) = u32::try_from(key).ok().and_then(char::from_u32)
                        && c.is_ascii_alphabetic()
                    {
                        used = g.sequence_key(c);
                    }
                });
            }
            if used {
                p.refresh(false, None);
            } else {
                e.skip(true);
            }
        });
    }
}

impl Gui {
    /// Redraws the active view.
    fn refresh_active(&self) {
        if let Some(v) = self.active_view() {
            v.panel.refresh(false, None);
        }
    }
}

/// Runs the main window until it is closed, opening `file` if given.
pub fn run(file: Option<PathBuf>) -> Result<(), String> {
    wxdragon::main(move |_| {
        let frame = Frame::builder()
            .with_title("NoDesk Reviera")
            .with_size(Size::new(1400, 860))
            .build();

        let file_menu = Menu::builder()
            .append_item(ID_NEW, "&New\tCtrl+N", "Create a project")
            .append_item(ID_OPEN, "&Open...\tCtrl+O", "Open an IFC project")
            .append_item(ID_SAVE_FILE, "&Save\tCtrl+S", "Save the project")
            .append_item(
                ID_SAVE_AS,
                "Save &As...\tCtrl+Shift+S",
                "Save the project to a new file",
            )
            .append_separator()
            .append_item(ID_EXIT, "E&xit", "Close Reviera")
            .build();
        let mut orient_menu = Menu::builder();
        for (k, (label, _)) in nav::PRESETS.iter().enumerate() {
            orient_menu = orient_menu.append_item(ID_ORIENT_FIRST + k as i32, label, "");
        }
        let edit_menu = Menu::builder()
            .append_item(ID_UNDO, "&Undo\tCtrl+Z", "Undo the last change")
            .append_item(ID_REDO, "&Redo\tCtrl+Y", "Redo the last undone change")
            .build();
        // Keyboard sequences (ZF, ZP, ...) are typed in the view (§1.26.4); single-letter menu
        // accelerators would take letters away from typed lengths.
        let view_menu = Menu::builder()
            .append_item(
                ID_DEFAULT_3D,
                "&Default 3D View",
                "Open the model's default 3D view",
            )
            .append_item(
                ID_NEW_PLAN,
                "New Floor &Plan...",
                "Create floor plans of levels",
            )
            .append_separator()
            .append_item(
                ID_FIT,
                "Zoom to &Fit (ZF)",
                "Show the whole model, or the selection",
            )
            .append_item(
                ID_ZOOM_PREV,
                "&Previous Pan/Zoom (ZP)",
                "Go back to the previous view",
            )
            .append_item(ID_ZOOM_NEXT, "&Next Pan/Zoom (ZN)", "Go forward again")
            .append_separator()
            .append_item(
                ID_HOME,
                "&Home\tAlt+Home",
                "Go to the view's home orientation",
            )
            .append_item(ID_SET_HOME, "Set Current View as Home", "")
            .build();
        view_menu.append_submenu(
            orient_menu.build(),
            "&Orient",
            "Standard and isometric views",
        );
        view_menu.append(
            ID_PERSPECTIVE,
            "Perspective / &Isometric (PI)",
            "Switch between perspective and isometric (parallel) projection",
            ItemKind::Normal,
        );
        view_menu.append_separator();
        view_menu.append(
            ID_WALK,
            "&Walk (WK)",
            "Walk through the model at eye height",
            ItemKind::Normal,
        );
        let architecture_menu = Menu::builder()
            .append_item(ID_WALL, "&Wall (WA)", "Draw walls on the lowest level")
            .build();
        let window_menu = Menu::builder()
            .append_item(
                ID_NEW_VIEW,
                "New &3D View",
                "Open another 3D view of the model",
            )
            .append_item(
                ID_CLOSE_VIEW,
                "&Close View\tCtrl+W",
                "Close the active view",
            )
            .append_separator()
            .append_item(
                ID_NEXT_VIEW,
                "&Next View\tCtrl+PgDn",
                "Activate the next view",
            )
            .append_item(
                ID_PREV_VIEW,
                "&Previous View\tCtrl+PgUp",
                "Activate the previous view",
            )
            .append_separator()
            .append_item(ID_TILE, "&Tile Views", "Show every view at once")
            .append_item(
                ID_TAB_ALL,
                "T&ab Views",
                "Put every view back in one tab group",
            )
            .append_separator()
            .append_item(
                ID_SHOW_PROPERTIES,
                "&Properties",
                "Show or hide the Properties palette",
            )
            .append_item(
                ID_SHOW_BROWSER,
                "Project &Browser",
                "Show or hide the Project Browser",
            )
            .build();
        let menu_bar = MenuBar::builder()
            .append(file_menu, "&File")
            .append(edit_menu, "&Edit")
            .append(view_menu, "&View")
            .append(architecture_menu, "&Architecture")
            .append(window_menu, "&Window")
            .build();
        frame.set_menu_bar(menu_bar);
        frame.create_status_bar(2, 0, -1, "status");
        frame.set_status_text("Open an IFC file to begin (File > Open)", 0);

        // Docking layout: Properties on the left, view tabs in the centre (Ch 1 §1.4).
        let aui = AuiManager::builder(&frame).build();
        let grid = PropertyGrid::builder(&frame)
            .with_style(PropertyGridStyle::Tooltips)
            .build();
        let notebook = AuiNotebook::builder(&frame).build();
        let browser = TreeCtrl::builder(&frame)
            .with_style(
                TreeCtrlStyle::Default | TreeCtrlStyle::HasButtons | TreeCtrlStyle::LinesAtRoot,
            )
            .build();
        aui.add_pane_with_info(
            &grid,
            AuiPaneInfo::new()
                .with_name("properties")
                .with_caption("Properties")
                .left()
                .layer(1)
                .best_size(340, 600)
                .min_size(220, 200)
                .close_button(true)
                .floatable(true),
        );
        aui.add_pane_with_info(
            &browser,
            AuiPaneInfo::new()
                .with_name("browser")
                .with_caption("Project Browser")
                .left()
                .layer(1)
                .row(0)
                .position(1)
                .best_size(340, 260)
                .min_size(220, 120)
                .close_button(true)
                .floatable(true),
        );
        aui.add_pane_with_info(
            &notebook,
            AuiPaneInfo::new().with_name("views").center_pane(),
        );
        aui.update();

        let gui = Rc::new_cyclic(|this| {
            RefCell::new(Gui {
                app: App::new(AppConfig::default()),
                this: this.clone(),
                frame,
                notebook,
                aui,
                doc: None,
                views: Vec::new(),
                active: None,
                next_view: 1,
                gpu: None,
                scenes: HashMap::new(),
                rows: Vec::new(),
                properties_stale: true,
                tool: None,
                timer: Some(Timer::new(&frame)),
                loading: Vec::new(),
                load_timer: Some(Timer::new(&frame)),
                browser,
                browser_items: Vec::new(),
                browser_stale: true,
                browser_target: None,
                plan_edges: HashMap::new(),
                keys: (String::new(), std::time::Instant::now()),
            })
        });
        {
            let g = gui.clone();
            let state = gui.borrow();
            state
                .load_timer
                .as_ref()
                .expect("created")
                .on_tick(move |_| {
                    with(&g, |g| {
                        if !g.poll_loading()
                            && let Some(t) = &g.load_timer
                        {
                            t.stop();
                        }
                    });
                });
        }
        {
            let g = gui.clone();
            let timer = gui.borrow();
            timer.timer.as_ref().expect("created").on_tick(move |_| {
                with(&g, |g| {
                    if !g.tick()
                        && let Some(t) = &g.timer
                    {
                        t.stop();
                    }
                });
            });
        }

        {
            let gui = gui.clone();
            wx::on_tab_event(&notebook, move |event| {
                with(&gui, |g| match event {
                    wx::TabEvent::Changed(i) => {
                        if let Some(v) = g.view_at(i) {
                            g.activate_view(v);
                        }
                    }
                    wx::TabEvent::Close(i) | wx::TabEvent::MiddleClick(i) => {
                        if let Some(v) = g.view_at(i) {
                            g.close_view(v);
                        } else {
                            g.cancel_loading(i);
                        }
                    }
                    wx::TabEvent::RightClick(_) => {}
                });
            });
        }
        {
            let gui = gui.clone();
            browser.on_item_activated(move |e| {
                if let Some(item) = e.get_item() {
                    with(&gui, |g| {
                        if let Some(view) = g.browser_view(&item) {
                            g.open_model_view(view);
                        }
                    });
                }
            });
        }
        {
            let (gui, b) = (gui.clone(), browser);
            browser.on_item_right_click(move |e| {
                let Some(item) = e.get_item() else { return };
                let mut target = None;
                with(&gui, |g| {
                    target = g.browser_view(&item);
                    g.browser_target = target;
                });
                if target.is_some() {
                    let mut menu = Menu::builder()
                        .append_item(ID_BROWSER_OPEN, "&Open", "")
                        .append_item(ID_BROWSER_RENAME, "&Rename...", "")
                        .append_item(ID_BROWSER_DUPLICATE, "&Duplicate", "")
                        .append_separator()
                        .append_item(ID_BROWSER_DELETE, "De&lete", "")
                        .build();
                    b.popup_menu(&mut menu, None);
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
                    gui.update_captions();
                }
                if let Ok(mut gui) = gui.try_borrow_mut() {
                    gui.sync_views();
                    if gui.browser_stale {
                        gui.rebuild_browser();
                    }
                }
                e.skip(true);
            });
        }
        {
            let gui = gui.clone();
            frame.on_close(move |e| {
                let mut closed = false;
                with(&gui, |g| closed = g.close_all());
                if closed {
                    e.skip(true);
                } else if let WindowEventData::General(e) = &e {
                    e.veto();
                }
            });
        }
        {
            let (gui, f) = (gui.clone(), frame);
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
                    }
                }
                ID_NEW => with(&gui, |g| g.new_project(&f)),
                ID_SAVE_FILE | ID_SAVE_AS => {
                    // A project that was never saved has no file yet: Save asks for one.
                    let mut current = None;
                    with(&gui, |g| {
                        if g.doc.is_some() {
                            current = Some(g.current_path());
                        }
                    });
                    let Some(current) = current else { return };
                    if e.get_id() == ID_SAVE_FILE && current.is_some() {
                        return with(&gui, |g| {
                            g.save(&f);
                            g.update_captions();
                        });
                    }
                    let name = current
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or("Untitled.ifc".into(), |n| n.to_string_lossy().into_owned());
                    let mut dialog = FileDialog::builder(&f)
                        .with_message("Save IFC project as")
                        .with_wildcard("IFC files (*.ifc)|*.ifc;*.IFC")
                        .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                        .with_default_file(&name);
                    if let Some(dir) = current.as_ref().and_then(|p| p.parent()) {
                        dialog = dialog.with_default_dir(&dir.to_string_lossy());
                    }
                    let dialog = dialog.build();
                    if dialog.show_modal() == ID_OK
                        && let Some(path) = dialog.get_path()
                    {
                        with(&gui, |g| {
                            g.save_as(Path::new(&path), &f);
                            g.update_captions();
                        });
                    }
                }
                ID_UNDO | ID_REDO => {
                    let command = if e.get_id() == ID_UNDO {
                        "edit.undo"
                    } else {
                        "edit.redo"
                    };
                    with(&gui, |g| g.undo_redo(command, &f));
                }
                ID_WALL => with(&gui, |g| {
                    g.start_wall_tool(&f);
                    if let Some(v) = g.active_view() {
                        v.panel.set_focus();
                    }
                }),
                ID_FIT => with(&gui, Gui::zoom_to_fit),
                ID_ZOOM_PREV => with(&gui, |g| g.history(true)),
                ID_ZOOM_NEXT => with(&gui, |g| g.history(false)),
                ID_HOME => with(&gui, Gui::go_home),
                ID_SET_HOME => with(&gui, Gui::set_home),
                ID_PERSPECTIVE => with(&gui, Gui::toggle_perspective),
                ID_WALK => with(&gui, Gui::toggle_walk),
                id if (ID_ORIENT_FIRST..ID_ORIENT_FIRST + nav::PRESETS.len() as i32)
                    .contains(&id) =>
                {
                    let dir = nav::PRESETS[(id - ID_ORIENT_FIRST) as usize].1;
                    with(&gui, |g| g.orient(glam::Vec3::from(dir)));
                }
                ID_NEW_VIEW => with(&gui, Gui::new_3d_view),
                ID_DEFAULT_3D => with(&gui, Gui::default_3d_view),
                ID_NEW_PLAN => with(&gui, Gui::new_floor_plans),
                ID_BROWSER_OPEN | ID_BROWSER_RENAME | ID_BROWSER_DUPLICATE | ID_BROWSER_DELETE => {
                    let id = e.get_id();
                    with(&gui, |g| {
                        let Some(view) = g.browser_target.take() else {
                            return;
                        };
                        match id {
                            ID_BROWSER_OPEN => g.open_model_view(view),
                            ID_BROWSER_RENAME => g.rename_view(view),
                            ID_BROWSER_DUPLICATE => g.duplicate_view(view),
                            _ => g.delete_view(view),
                        }
                    });
                }
                ID_CLOSE_VIEW => with(&gui, |g| {
                    if let Some(v) = g.active {
                        g.close_view(v);
                    }
                }),
                ID_NEXT_VIEW => with(&gui, |g| g.cycle_view(1)),
                ID_PREV_VIEW => with(&gui, |g| g.cycle_view(-1)),
                ID_TILE => with(&gui, Gui::tile),
                ID_TAB_ALL => with(&gui, Gui::tab_all),
                ID_SHOW_PROPERTIES | ID_SHOW_BROWSER => {
                    let pane = if e.get_id() == ID_SHOW_PROPERTIES {
                        "properties"
                    } else {
                        "browser"
                    };
                    with(&gui, |g| {
                        let shown = g.aui.is_pane_shown(pane);
                        g.aui.set_pane_shown(pane, !shown);
                        g.aui.update();
                    });
                }
                ID_EXIT => f.close(false),
                _ => e.skip(true),
            });
        }

        frame.show(true);
        frame.centre();
        if let Some(path) = &file {
            gui.borrow_mut().open(path, &frame);
        }
    })
    .map_err(|e| e.to_string())
}
