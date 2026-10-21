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

    // Most building elements tessellate as closed solids, so section cuts can be capped.
    let solids = scene
        .meshes
        .iter()
        .filter(|m| reviera_render_wgpu::solid(m).is_some())
        .count();
    eprintln!(
        "{solids} of {} meshes are closed solids",
        scene.meshes.len()
    );
    assert!(solids * 2 > scene.meshes.len());

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

    // Picking through the centre of the view finds an element; highlighting it changes the picture.
    let (origin, dir) = camera.ray(320.0, 200.0, 640.0, 400.0);
    let (hit, _) = scene
        .pick(origin, dir)
        .expect("the centre of the view shows the house");
    let element = scene.meshes[hit]
        .element
        .expect("picked meshes map to elements");
    viewport.set_highlight(&[element]);
    viewport.render(&camera).unwrap();
    let highlighted = viewport.read_pixels().unwrap();
    let changed = pixels
        .iter()
        .zip(&highlighted)
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 100, "{changed} bytes changed");

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

#[test]
fn the_orientation_cube_draws_in_its_corner() {
    let mut viewport = match WgpuViewport::offscreen(640, 400) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("skipped: no graphics adapter ({e})");
            return;
        }
    };
    let camera = reviera_render::Camera::default();
    viewport.render(&camera).unwrap();
    let empty = viewport.read_pixels().unwrap();
    let cube = reviera_render::viewcube::ViewCube::default();
    viewport.set_gizmo(Some(cube.gizmo(&camera, 640.0, 400.0, None)));
    viewport.render(&camera).unwrap();
    let with_cube = viewport.read_pixels().unwrap();
    if let Ok(path) = std::env::var("REVIERA_DUMP_PPM") {
        let mut out = b"P6 640 400 255\n".to_vec();
        for px in with_cube.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(path, out).unwrap();
    }
    let [x, y, w, h] = cube.rect(640.0, 400.0);
    let pixel = |p: &[u8], px: f32, py: f32| {
        let i = ((py as usize) * 640 + px as usize) * 4;
        p[i..i + 3].to_vec()
    };
    // The cube's centre is drawn; the opposite corner of the view is untouched.
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    assert_ne!(pixel(&with_cube, cx, cy), pixel(&empty, cx, cy));
    assert_eq!(pixel(&with_cube, 20.0, 380.0), pixel(&empty, 20.0, 380.0));
}

#[test]
fn a_floor_plan_shows_cut_walls_dark_on_white() {
    let mut viewport = match WgpuViewport::offscreen(800, 600) {
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
    let ground = reviera_core::plans::levels(app.document(id).unwrap().doc.store())[0].0 as f32;
    let edges = reviera_render::plan::feature_edges(&scene);
    let weights = |_| reviera_render::plan::Weights {
        cut: Some(0.5),
        projection: 0.18,
        opacity: 1.0,
        above: reviera_core::categories::AboveCut::Hide,
    };
    let plan = reviera_render::plan::plan_graphics(
        &scene,
        &edges,
        ground + 1.2,
        ground - 1.22,
        100.0,
        &weights,
    );
    assert!(plan.poche.len() > 5, "walls are cut: {}", plan.poche.len());

    let mut camera = Camera {
        perspective: false,
        ..Camera::default()
    };
    camera.orient(glam::Vec3::Z);
    camera.fit(scene.bounds().unwrap());
    viewport.set_scene(&scene);
    viewport.set_plan(Some(plan));
    viewport.render(&camera).unwrap();
    let pixels = viewport.read_pixels().unwrap();
    if let Ok(path) = std::env::var("REVIERA_DUMP_PLAN") {
        let mut out = b"P6 800 600 255\n".to_vec();
        for px in pixels.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(path, out).unwrap();
    }
    let px = pixels.as_chunks::<4>().0;
    let white = px
        .iter()
        .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245)
        .count();
    let dark = px
        .iter()
        .filter(|p| p[0] < 90 && p[1] < 90 && p[2] < 90)
        .count();
    assert!(white > px.len() / 3, "paper background: {white}");
    assert!(dark > 1000, "cut poché and lines: {dark}");
}

#[test]
fn a_slice_plane_shows_a_capped_cut_and_hidden_elements_vanish() {
    let mut viewport = match WgpuViewport::offscreen(400, 400) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("skipped: no graphics adapter ({e})");
            return;
        }
    };
    // A 2 m cube, red, looked at from above.
    let square = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let (positions, indices) = reviera_render::solid::extrude(&square, &[], 0.0, 2.0);
    let cube = reviera_render::SceneMesh {
        element: Some(reviera_core::ElementId(7)),
        class: "IfcWall".into(),
        positions,
        indices,
        color: [0.8, 0.1, 0.1, 1.0],
    };
    let scene = reviera_render::RenderScene { meshes: vec![cube] };
    let mut camera = Camera {
        target: glam::Vec3::new(0.0, 0.0, 1.0),
        distance: 8.0,
        perspective: false,
        ..Camera::default()
    };
    camera.orient(glam::Vec3::new(0.0, -0.3, 1.0));
    viewport.set_scene(&scene);
    let centre = |v: &mut WgpuViewport| {
        v.render(&camera).unwrap();
        let p = v.read_pixels().unwrap();
        let i = (200 * 400 + 200) * 4;
        [p[i], p[i + 1], p[i + 2]]
    };
    let top = centre(&mut viewport);
    // Keep what is below 1 m: the camera now looks into the cube through the cut, at its inside.
    viewport.set_clipping(&reviera_render::Clipping {
        planes: vec![[0.0, 0.0, -1.0, -1.0]],
        caps: true,
    });
    let cut = centre(&mut viewport);
    eprintln!("top {top:?}, cut {cut:?}");
    assert_ne!(top, cut, "the cut changes the picture");
    // The cap is the solid's colour, darkened and unlit: red, and darker than the lit top.
    assert!(cut[0] > 2 * cut[1] && cut[1] == cut[2], "{cut:?}");
    assert!(u32::from(cut[0]) < u32::from(top[0]), "{cut:?} {top:?}");
    // Hiding the element shows the background.
    viewport.set_clipping(&reviera_render::Clipping::default());
    let mut hidden = reviera_render::Visibility::default();
    hidden.hidden.insert(reviera_core::ElementId(7));
    viewport.set_visibility(&hidden);
    let gone = centre(&mut viewport);
    assert!(gone[2] > gone[0], "background blue-grey: {gone:?}");
    // Translucent: lighter than opaque, still reddish.
    let mut faint = reviera_render::Visibility::default();
    faint.translucent.insert(reviera_core::ElementId(7), 0.25);
    viewport.set_visibility(&faint);
    let ghost = centre(&mut viewport);
    assert!(
        ghost != top && ghost != gone && ghost[0] > ghost[2],
        "{ghost:?}"
    );
}
