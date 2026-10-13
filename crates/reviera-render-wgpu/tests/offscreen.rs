// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Renders the FZK-Haus model offscreen and checks the picture.

use std::path::Path;

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use reviera_render::{Camera, RenderBackend};
use reviera_render_wgpu::WgpuViewport;
use serde_json::json;

#[test]
fn fzk_haus_renders_offscreen() {
    let mut viewport = match WgpuViewport::offscreen(640, 400) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("skipped: no graphics adapter ({e})");
            return;
        }
    };
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc/AC20-FZK-Haus.ifc");
    let id = app
        .execute(
            "file.open",
            json!({"path": file, "read_only": true}),
            &Caller::user(),
        )
        .unwrap()
        .result
        .as_u64()
        .unwrap();
    let scene = app.render_scene(id).unwrap();
    assert!(
        scene.meshes.len() > 50 && scene.triangle_count() > 10_000,
        "{} meshes",
        scene.meshes.len()
    );
    assert!(
        scene.meshes.iter().filter(|m| m.element.is_some()).count() > 50,
        "meshes map back to elements"
    );

    let mut camera = Camera::default();
    camera.fit(scene.bounds().unwrap());
    viewport.set_scene(&scene);
    viewport.render(&camera).unwrap();
    let pixels = viewport.read_pixels().unwrap();
    assert_eq!(pixels.len(), 640 * 400 * 4);
    // The model covers a good part of the frame, against the plain background.
    let background = &pixels[0..4];
    let covered = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p.as_slice() != background)
        .count();
    let share = covered as f64 / (640.0 * 400.0);
    eprintln!(
        "adapter: {}; model covers {:.0}% of the frame",
        viewport.adapter(),
        share * 100.0
    );
    assert!(share > 0.10 && share < 0.95, "{share}");

    if let Some(out) = std::env::var_os("REVIERA_RENDER_PNG") {
        let file = std::fs::File::create(out).unwrap();
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), 640, 400);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
}
