// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View images for agents (DM-MCP-004 `get_view_image`): a PNG of a view as it is now, so a
//! multimodal agent can look at what it did.
//!
//! - **With a window** (the agent terminal's sessions): the request is queued on the [`App`]
//!   and the window answers it after the agent's calls have run and the views have refreshed,
//!   capturing the view as the user sees it (its camera, plan graphics, annotation, selection
//!   highlight, section box), or rendering it offscreen when it is not open in a tab.
//! - **Headless** (`reviera --mcp-stdio`): rendered offscreen here: plans with their plan graphics
//!   and annotation from above, 3D views from their camera (or a south-east isometric of the whole
//!   model).

use std::sync::mpsc;

use reviera_core::{ElementId, Error, Result, plans, views};
use reviera_render::{Camera, RenderBackend};
use reviera_render_wgpu::WgpuViewport;

use crate::{App, DocumentId};

/// A captured view.
#[derive(Debug, Clone)]
pub struct ViewImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The view's name.
    pub view: String,
    /// `window` (as shown) or `offscreen`.
    pub source: &'static str,
}

/// A request for the window to capture a view.
#[derive(Debug)]
pub struct ImageRequest {
    pub doc: DocumentId,
    /// The view; the active one when none.
    pub view: Option<ElementId>,
    /// The longest side, pixels.
    pub max_size: u32,
    /// Keep the view cube and other on-screen controls.
    pub gizmos: bool,
    pub reply: mpsc::Sender<Result<ViewImage>>,
}

impl App {
    /// The window says it is there to capture views (it then answers [`ImageRequest`]s).
    pub fn set_window(&mut self, on: bool) {
        self.window = on;
    }

    pub fn has_window(&self) -> bool {
        self.window
    }

    /// Queues a capture for the window.
    pub fn request_view_image(&mut self, r: ImageRequest) {
        self.image_requests.push(r);
    }

    /// The captures asked for since the last call.
    pub fn take_image_requests(&mut self) -> Vec<ImageRequest> {
        std::mem::take(&mut self.image_requests)
    }

    /// Renders a plan or 3D view of `doc` offscreen (no window needed); `view` none: the active
    /// view, else a 3D view of the whole model.
    pub fn render_view_image(&self, doc: DocumentId, view: Option<ElementId>, max_size: u32) -> Result<ViewImage> {
        let store = self.get(doc)?.doc.store();
        let view = view.or_else(|| self.active_view(doc));
        let data = view.and_then(|v| store.get(v)).and_then(views::view_data);
        let name = view.and_then(|v| store.get(v)).and_then(|e| e.name.clone()).unwrap_or_else(|| "Model".into());
        let size = max_size.clamp(64, 4096);
        let (w, h) = (size, (size * 3 / 4).max(48));
        let mut vp = WgpuViewport::offscreen(w, h).map_err(|e| Error::new("view_image.gpu", format!("no graphics adapter to render with: {e}")))?;
        let scene = self.render_scene(doc)?;
        vp.set_scene(&scene);
        let aspect = w as f32 / h as f32;
        let camera = match (&data, view) {
            (Some(d), Some(v)) if plans::is_plan(&d.kind) => {
                let drawing = self.plan_drawing_of(&scene, doc, v)?;
                let ext = crate::output::extents(&drawing);
                vp.set_view_lines(&drawing.annotation);
                vp.set_plan(Some(drawing.graphics));
                match self.view_camera(doc, v) {
                    Some(c) => Camera::from_view(&c, aspect),
                    None => {
                        let (lo, hi) = ext.unwrap_or(([-10.0, -10.0], [10.0, 10.0]));
                        let c = views::ViewCamera {
                            eye: [f64::from(lo[0] + hi[0]) / 2.0, f64::from(lo[1] + hi[1]) / 2.0, 1000.0],
                            target: [f64::from(lo[0] + hi[0]) / 2.0, f64::from(lo[1] + hi[1]) / 2.0, 0.0],
                            up: [0.0, 1.0, 0.0],
                            perspective: false,
                            field_of_view: 0.8,
                            parallel_scale: f64::from(((hi[1] - lo[1]) / 2.0).max((hi[0] - lo[0]) / 2.0 / aspect)) * 1.1,
                        };
                        Camera::from_view(&c, aspect)
                    }
                }
            }
            (Some(d), Some(v)) if d.kind == views::KIND_3D => {
                // As the view shows it: its section box and slices, its temporary hide and
                // isolate, parts or their originals.
                vp.set_clipping(&reviera_render::Clipping::of(d.section_box.as_ref(), &d.slice_planes));
                let mut vis = self.temp_state(doc, Some(v)).visibility();
                vis.hidden.extend(reviera_core::parts::hidden_in(store, Some(v)));
                vp.set_visibility(&vis);
                let c = self.view_camera(doc, v).unwrap_or_else(|| self.fitting_camera(doc));
                Camera::from_view(&c, aspect)
            }
            (Some(d), _) => {
                return Err(Error::new(
                    "view_image.kind",
                    format!("{name} is a {} view: capture it from the window (open it there), or ask for a plan or 3D view", d.kind.trim_start_matches("view.")),
                ));
            }
            _ => Camera::from_view(&self.fitting_camera(doc), aspect),
        };
        vp.render(&camera).map_err(|e| Error::new("view_image.render", e))?;
        let px = vp.read_pixels().ok_or_else(|| Error::new("view_image.render", "the frame could not be read back"))?;
        let png = reviera_render_wgpu::encode_png(w, h, &px).map_err(|e| Error::new("view_image.png", e))?;
        Ok(ViewImage { png, width: w, height: h, view: name, source: "offscreen" })
    }

    /// A BCF viewpoint rendered offscreen (COL-BCF-001): its camera, its clipping planes as the
    /// section box and slices they form, its selection highlighted and its hidden or isolated
    /// components, as a PNG.
    pub fn render_viewpoint(&self, doc: DocumentId, vp: &reviera_core::bcf::Viewpoint, max_size: u32) -> Result<ViewImage> {
        use reviera_core::id::GlobalId;
        let store = self.get(doc)?.doc.store();
        let size = max_size.clamp(64, 4096);
        let (w, h) = (size, (size * 3 / 4).max(48));
        let mut viewport = WgpuViewport::offscreen(w, h).map_err(|e| Error::new("view_image.gpu", format!("no graphics adapter to render with: {e}")))?;
        let scene = self.render_scene(doc)?;
        viewport.set_scene(&scene);
        let (section_box, slices) = views::from_bcf(&vp.clipping_planes);
        viewport.set_clipping(&reviera_render::Clipping::of(section_box.as_ref(), &slices));
        let find = |gs: &[String]| -> Vec<ElementId> { gs.iter().filter_map(|g| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))).collect() };
        viewport.set_highlight(&find(&vp.selection));
        let exceptions = find(&vp.visibility.exceptions);
        let mut vis = reviera_render::Visibility::default();
        if vp.visibility.default_visibility {
            vis.hidden.extend(exceptions);
        } else {
            let keep: std::collections::HashSet<ElementId> = exceptions.into_iter().collect();
            vis.hidden.extend(store.iter().map(|e| e.id()).filter(|id| !keep.contains(id)));
        }
        viewport.set_visibility(&vis);
        let c = match &vp.camera {
            Some(c) => reviera_core::bcf::view_camera(c, 10.0)?,
            None => self.fitting_camera(doc),
        };
        viewport.render(&Camera::from_view(&c, w as f32 / h as f32)).map_err(|e| Error::new("view_image.render", e))?;
        let px = viewport.read_pixels().ok_or_else(|| Error::new("view_image.render", "the frame could not be read back"))?;
        let png = reviera_render_wgpu::encode_png(w, h, &px).map_err(|e| Error::new("view_image.png", e))?;
        Ok(ViewImage { png, width: w, height: h, view: "viewpoint".into(), source: "offscreen" })
    }
}
