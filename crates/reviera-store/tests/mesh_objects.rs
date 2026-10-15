// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom objects from meshes (DM-IMP-015): importing an OBJ with materials, placing copies,
//! the modify tools, and IFC (one representation map, mapped copies), reopened.

use reviera_core::mesh_objects;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

/// A 45 × 50 × 90 cm "chair": a seat box (wood) and a back box (fabric), in centimetres.
fn write_chair(dir: &std::path::Path) -> std::path::PathBuf {
    let mut obj = String::from("mtllib chair.mtl\n");
    let mut n = 0;
    let mut boxed = |obj: &mut String, lo: [f64; 3], hi: [f64; 3], mat: &str| {
        for z in [lo[2], hi[2]] {
            for (x, y) in [
                (lo[0], lo[1]),
                (hi[0], lo[1]),
                (hi[0], hi[1]),
                (lo[0], hi[1]),
            ] {
                obj.push_str(&format!("v {x} {y} {z}\n"));
            }
        }
        obj.push_str(&format!("usemtl {mat}\n"));
        let q = |a: usize, b: usize, c: usize, d: usize| {
            format!("f {} {} {} {}\n", n + a, n + b, n + c, n + d)
        };
        for f in [
            q(1, 4, 3, 2),
            q(5, 6, 7, 8),
            q(1, 2, 6, 5),
            q(2, 3, 7, 6),
            q(3, 4, 8, 7),
            q(4, 1, 5, 8),
        ] {
            obj.push_str(&f);
        }
        n += 8;
    };
    boxed(&mut obj, [0.0, 0.0, 0.0], [45.0, 50.0, 45.0], "wood");
    boxed(&mut obj, [0.0, 45.0, 45.0], [45.0, 50.0, 90.0], "fabric");
    std::fs::write(dir.join("chair.obj"), obj).unwrap();
    std::fs::write(
        dir.join("chair.mtl"),
        "newmtl wood\nKd 0.6 0.4 0.2\nnewmtl fabric\nKd 0.1 0.2 0.7\n",
    )
    .unwrap();
    dir.join("chair.obj")
}

#[test]
fn obj_chair_imported_placed_mirrored_and_saved_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ifc");
    let chair = write_chair(dir.path());
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // Inspection, in centimetres: about 0.45 m wide, two materials, closed boxes.
    let report = run(
        &r,
        &mut doc,
        "mesh_object.inspect",
        json!({ "path": chair, "options": { "units": "cm" } }),
    )
    .unwrap();
    assert!(
        (report["size"][0].as_f64().unwrap() - 0.45).abs() < 1e-9,
        "{report}"
    );
    assert_eq!(report["materials"], json!(["wood", "fabric"]));
    assert_eq!(report["open_edges"], 0);

    let t = id(run(
        &r,
        &mut doc,
        "mesh_object.import",
        json!({ "path": chair, "category": "furniture", "options": { "units": "cm" } }),
    )
    .unwrap());
    let ty = doc.store().get(t).unwrap();
    assert_eq!(ty.class, "IfcFurnitureType");
    assert_eq!(ty.name.as_deref(), Some("chair"));
    let recipe = mesh_objects::mesh_object_type(ty).unwrap();
    assert_eq!(recipe.slots.len(), 2);
    assert!((recipe.slots[1].color[2] - 0.7).abs() < 1e-6);

    // Three copies, one turned and one mirrored.
    let a = id(run(
        &r,
        &mut doc,
        "mesh_object.place",
        json!({ "type_id": t.0, "point": [1.0, 1.0], "level": l1.0 }),
    )
    .unwrap());
    let b = id(run(
        &r,
        &mut doc,
        "mesh_object.place",
        json!({ "type_id": t.0, "point": [3.0, 1.0], "level": l1.0, "rotation": "90°" }),
    )
    .unwrap());
    let c = id(run(&r, &mut doc, "mesh_object.place", json!({ "type_id": t.0, "point": [5.0, 1.0], "level": l1.0, "mirror": true, "offset": 0.5 })).unwrap());
    assert_eq!(doc.store().get(a).unwrap().class, "IfcFurniture");
    let bodies = reviera_core::bodies::all(doc.store());
    let body = |e: ElementId| bodies.iter().find(|b| b.element == e).unwrap().clone();
    // Two parts (wood, fabric), each in its colour.
    assert_eq!(body(a).parts.len(), 2);
    assert!(body(a).parts[0].class.starts_with("IfcFurniture:#"));
    let (lo, hi) = body(a).bounds().unwrap();
    assert!(
        (lo[0] - 0.775).abs() < 1e-6 && (hi[0] - 1.225).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Turned 90°: its 45 cm now runs along y.
    let (lo, hi) = body(b).bounds().unwrap();
    assert!((hi[1] - lo[1] - 0.45).abs() < 1e-6);
    // Mirrored across its x axis: the back is now at -y; raised 0.5 m.
    let (lo, hi) = body(c).bounds().unwrap();
    assert!((lo[2] - 0.5).abs() < 1e-6 && (hi[2] - 1.4).abs() < 1e-6);
    let back_y = |e: ElementId| {
        let parts = body(e).parts;
        let back = &parts[1].mesh.0;
        back.iter().map(|p| p[1]).sum::<f64>() / back.len() as f64
    };
    assert!(back_y(a) > 1.0 && back_y(c) < 1.0);

    // Moving and mirroring with the modify tools.
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [a.0], "vector": [0.0, 2.0, 0.0] }),
    )
    .unwrap();
    let rec = mesh_objects::mesh_object_recipe(doc.store().get(a).unwrap()).unwrap();
    assert_eq!(rec.point, [1.0, 3.0]);
    run(&r, &mut doc, "element.mirror", json!({ "element_ids": [a.0], "axis": { "start": [0.0, 0.0], "end": [10.0, 0.0] }, "copy": false })).unwrap();
    let rec = mesh_objects::mesh_object_recipe(doc.store().get(a).unwrap()).unwrap();
    assert!(rec.mirror && (rec.point[1] + 3.0).abs() < 1e-9);

    // The type's colour and scale change every copy.
    run(
        &r,
        &mut doc,
        "mesh_object.set_type",
        json!({ "type_id": t.0, "scale": 2.0, "colors": [{ "slot": "fabric", "color": [1.0, 0.0, 0.0, 1.0] }] }),
    )
    .unwrap();
    let bodies = reviera_core::bodies::all(doc.store());
    let (lo, hi) = bodies
        .iter()
        .find(|x| x.element == b)
        .unwrap()
        .bounds()
        .unwrap();
    assert!((hi[2] - lo[2] - 1.8).abs() < 1e-6);
    let listed = run(&r, &mut doc, "mesh_object.types", json!({})).unwrap();
    assert_eq!(listed[0]["copies"], 3);

    // IFC: one furniture type with one map, three furniture copies mapping it.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCFURNITURETYPE"), 1);
    assert_eq!(count("IFCFURNITURE"), 3);
    assert_eq!(count("IFCREPRESENTATIONMAP"), 1);
    assert_eq!(count("IFCMAPPEDITEM"), 3);
    assert_eq!(count("IFCTRIANGULATEDFACESET"), 2);
    assert_eq!(count("IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM"), 2); // a and c are mirrored
    // Other IFC tools see the copies: IfcOpenShell's geometry engine tessellates each one, at its
    // place and scale (the doubled chair is 1.8 m tall).
    let meshes = IfcModel::open(&path).unwrap().tessellate(1).unwrap();
    let chairs: Vec<_> = meshes
        .iter()
        .filter(|m| m.class == "IfcFurniture")
        .collect();
    assert_eq!(chairs.len(), 3);
    for m in &chairs {
        assert_eq!(m.triangle_count(), 24);
        let zs: Vec<f64> = m.verts.chunks(3).map(|v| v[2]).collect();
        let h = zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - zs.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!((h - 1.8).abs() < 1e-3, "height {h}");
    }
    // A change elsewhere rewrites only what changed: the map and copies keep their entities.
    run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap();
    project.save(&doc).unwrap();
    let rows_again = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let ids = |rows: &[reviera_ifc::StepRow], class: &str| -> Vec<u32> {
        rows.iter()
            .filter(|r| r.class == class)
            .map(|r| r.id.0)
            .collect()
    };
    assert_eq!(
        ids(&rows_again, "IFCMAPPEDITEM"),
        ids(&rows, "IFCMAPPEDITEM")
    );
    assert_eq!(
        ids(&rows_again, "IFCREPRESENTATIONMAP"),
        ids(&rows, "IFCREPRESENTATIONMAP")
    );
    drop(project);

    // Reopened: the type and its three copies regenerate with the same bodies.
    let (mut p2, mut doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(mesh_objects::all(doc2.store()).len(), 3);
    let b2 = reviera_core::bodies::all(doc2.store());
    assert_eq!(
        b2.iter()
            .filter(|x| x
                .parts
                .iter()
                .any(|p| p.class.starts_with("IfcFurniture:#")))
            .count(),
        3
    );

    // Deleting the type deletes its copies, and their geometry leaves the file.
    let t2 = doc2
        .store()
        .iter()
        .find(|e| mesh_objects::mesh_object_type(e).is_some())
        .unwrap()
        .id();
    run(
        &r,
        &mut doc2,
        "element.delete",
        json!({ "element_ids": [t2.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert!(mesh_objects::all(doc2.store()).is_empty());
    p2.save(&doc2).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCFURNITURE"), 0);
    assert_eq!(count("IFCMAPPEDITEM"), 0);
    assert_eq!(count("IFCTRIANGULATEDFACESET"), 0);
    assert_eq!(count("IFCCARTESIANPOINTLIST3D"), 0);
}

#[test]
fn glb_imports_upright_in_metres_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    // A 2 m tall triangle standing in glTF's Y-up space.
    let mut buf = Vec::new();
    for v in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]] {
        for c in v {
            buf.extend(c.to_le_bytes());
        }
    }
    let json = json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": buf.len()}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "nodes": [{"mesh": 0}], "scenes": [{"nodes": [0]}]
    })
    .to_string()
    .into_bytes();
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    glb.extend(0u32.to_le_bytes());
    let pad = |v: &mut Vec<u8>, b: u8| {
        while !v.len().is_multiple_of(4) {
            v.push(b)
        }
    };
    let mut j = json;
    pad(&mut j, b' ');
    glb.extend((j.len() as u32).to_le_bytes());
    glb.extend(0x4E4F534Au32.to_le_bytes());
    glb.extend(&j);
    glb.extend((buf.len() as u32).to_le_bytes());
    glb.extend(0x004E4942u32.to_le_bytes());
    glb.extend(&buf);
    let file = dir.path().join("sign.glb");
    std::fs::write(&file, &glb).unwrap();

    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &dir.path().join("g.ifc")).unwrap();
    let r = registry();
    let report = run(&r, &mut doc, "mesh_object.inspect", json!({ "path": file })).unwrap();
    assert_eq!(report["units_from_file"], true);
    assert_eq!(report["y_up"], true);
    // Upright: 2 m tall in z.
    assert!(
        (report["size"][2].as_f64().unwrap() - 2.0).abs() < 1e-6,
        "{report}"
    );
    let t = id(run(&r, &mut doc, "mesh_object.import", json!({ "path": file })).unwrap());
    assert_eq!(
        doc.store().get(t).unwrap().class,
        "IfcBuildingElementProxyType"
    );
    // Reloaded as Z up instead: now 2 m deep.
    run(
        &r,
        &mut doc,
        "mesh_object.reload",
        json!({ "type_id": t.0, "options": { "up_axis": "z" } }),
    )
    .unwrap();
    let ty = mesh_objects::mesh_object_type(doc.store().get(t).unwrap()).unwrap();
    assert!((ty.size[1] - 2.0).abs() < 1e-6 && ty.size[2].abs() < 1e-6);
}
