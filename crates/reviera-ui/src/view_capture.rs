// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View images for the agent terminal's sessions (DM-MCP-004 `get_view_image`): after the
//! agents' calls have run and the views have refreshed, each requested view is captured as the
//! user sees it (camera, plan graphics, annotation, selection, section box), without the view
//! cube unless asked; a view not open in a tab is rendered offscreen.

use reviera_app::view_image::{ImageRequest, ViewImage};
use reviera_core::{Error, Result};

use super::Gui;

impl Gui {
    /// Answers the captures the agents asked for.
    pub(crate) fn serve_image_requests(&mut self) {
        for r in self.app.take_image_requests() {
            let out = self.capture_request(&r);
            let _ = r.reply.send(out);
        }
    }

    fn capture_request(&mut self, r: &ImageRequest) -> Result<ViewImage> {
        let index = match r.view {
            Some(el) => self.views.iter().position(|v| v.doc == r.doc && v.element == Some(el)),
            None => self.active.and_then(|id| self.views.iter().position(|v| v.id == id && v.doc == r.doc)),
        }
        .filter(|i| self.views[*i].schedule.is_none() && self.views[*i].viewport.is_some());
        let Some(index) = index else {
            return self.app.render_view_image(r.doc, r.view, r.max_size);
        };
        self.update_plan_graphics(index);
        self.update_section_graphics(index);
        self.update_sheet_graphics(index);
        self.update_inspection(index);
        let name = self.views[index]
            .element
            .and_then(|e| self.app.document(r.doc)?.doc.store().get(e)?.name.clone())
            .unwrap_or_else(|| "View".into());
        let view = &mut self.views[index];
        let camera = view.camera;
        let vp = view.viewport.as_mut().expect("filtered");
        let (w, h, px) = vp.capture(&camera, r.gizmos).map_err(|e| Error::new("view_image.render", e))?;
        let (w, h, px) = reviera_render_wgpu::fit_image(w, h, px, r.max_size);
        let png = reviera_render_wgpu::encode_png(w, h, &px).map_err(|e| Error::new("view_image.png", e))?;
        Ok(ViewImage { png, width: w, height: h, view: name, source: "window" })
    }
}
