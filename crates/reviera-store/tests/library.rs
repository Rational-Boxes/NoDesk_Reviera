// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Component Libraries (DM-LIB-002 … 004): a folder with a reviera-library.json, Import into
//! Library with companion files, details in the manifest, folder settings, Place Component from a
//! library item (loaded once), and deleting and restoring items.

use reviera_core::mesh_objects;
use reviera_core::{Caller, CommandRegistry, Document};
use reviera_ifc::Schema;
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

/// A 40 cm cube in centimetres, with a material, as OBJ + MTL in `dir`.
fn write_cube(dir: &std::path::Path, stem: &str) -> std::path::PathBuf {
    let mut obj = format!("mtllib {stem}.mtl\n");
    for z in [0, 40] {
        for (x, y) in [(0, 0), (40, 0), (40, 40), (0, 40)] {
            obj.push_str(&format!("v {x} {y} {z}\n"));
        }
    }
    obj.push_str("usemtl oak\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n");
    std::fs::write(dir.join(format!("{stem}.obj")), obj).unwrap();
    std::fs::write(
        dir.join(format!("{stem}.mtl")),
        "newmtl oak\nKd 0.7 0.5 0.3\n",
    )
    .unwrap();
    dir.join(format!("{stem}.obj"))
}

#[test]
fn library_import_details_place_delete_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("downloads");
    std::fs::create_dir_all(&src).unwrap();
    let stool = write_cube(&src, "stool");
    let lib = tmp.path().join("Office Library");
    let (_p, mut doc) = LocalProject::new(Schema::Ifc4, &tmp.path().join("p.ifc")).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A folder without a manifest is not a library until one is made.
    std::fs::create_dir_all(&lib).unwrap();
    assert!(run(&r, &mut doc, "library.open", json!({ "path": lib })).is_err());
    let info = run(
        &r,
        &mut doc,
        "library.create",
        json!({ "path": lib, "name": "Office" }),
    )
    .unwrap();
    assert_eq!(info["name"], "Office");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(lib.join("reviera-library.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["format"], "reviera-library");

    // Folder settings: Furniture items are furniture, in centimetres.
    run(
        &r,
        &mut doc,
        "library.set_meta",
        json!({ "library": lib, "paths": ["Furniture/"], "settings": { "category": "furniture", "import": { "units": "cm" } } }),
    )
    .unwrap();

    // Import into Library copies the OBJ and its MTL; the same content again is skipped.
    let report = run(
        &r,
        &mut doc,
        "library.import_files",
        json!({ "library": lib, "folder": "Furniture/Seating", "files": [stool], "details": { "tags": ["oak"] } }),
    )
    .unwrap();
    assert_eq!(report["imported"][0]["path"], "Furniture/Seating/stool.obj");
    assert_eq!(
        report["imported"][0]["companions"],
        json!(["Furniture/Seating/stool.mtl"])
    );
    assert!(lib.join("Furniture/Seating/stool.mtl").is_file());
    let again = run(
        &r,
        &mut doc,
        "library.import_files",
        json!({ "library": lib, "folder": "Furniture/Seating", "files": [stool] }),
    )
    .unwrap();
    assert!(again["imported"].as_array().unwrap().is_empty());
    assert!(
        again["skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("already in the library")
    );
    // A non-3D file is refused.
    std::fs::write(src.join("notes.txt"), "x").unwrap();
    let txt = run(
        &r,
        &mut doc,
        "library.import_files",
        json!({ "library": lib, "files": [src.join("notes.txt")] }),
    )
    .unwrap();
    assert_eq!(txt["skipped"].as_array().unwrap().len(), 1);

    // A file dropped in by a file manager is an item too.
    std::fs::create_dir_all(lib.join("Planting")).unwrap();
    write_cube(&lib.join("Planting"), "box shrub");

    // Listing: folders with counts, items with inherited details.
    let top = run(&r, &mut doc, "library.list", json!({ "library": lib })).unwrap();
    let folders: Vec<(String, u64)> = top["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().to_owned(),
                f["items"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        folders,
        vec![("Furniture".to_owned(), 1), ("Planting".to_owned(), 1)]
    );
    let seating = run(
        &r,
        &mut doc,
        "library.list",
        json!({ "library": lib, "folder": "Furniture/Seating" }),
    )
    .unwrap();
    let item = &seating["items"][0];
    assert_eq!(item["category"], "furniture");
    assert_eq!(item["tags"], json!(["oak"]));
    assert_eq!(item["name"], "stool");

    // Details the format cannot hold, kept in the manifest; the file is unchanged.
    let before = std::fs::read(lib.join("Furniture/Seating/stool.obj")).unwrap();
    run(
        &r,
        &mut doc,
        "library.set_meta",
        json!({ "library": lib, "paths": ["Furniture/Seating/stool.obj"], "details": { "name": "Oak Stool", "product": { "manufacturer": "Woodworks" } } }),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(lib.join("Furniture/Seating/stool.obj")).unwrap(),
        before
    );
    let meta = run(
        &r,
        &mut doc,
        "library.get_meta",
        json!({ "library": lib, "item": "Furniture/Seating/stool.obj" }),
    )
    .unwrap();
    assert_eq!(meta["details"]["name"], "Oak Stool");
    assert_eq!(meta["details"]["product"]["manufacturer"], "Woodworks");
    assert_eq!(meta["folder"]["import"]["units"], "cm");

    // Place Component from the library: loaded once as a type (in cm, as furniture), reused.
    let first = run(
        &r,
        &mut doc,
        "component.place",
        json!({ "library": lib, "item": "Furniture/Seating/stool.obj", "points": [[0, 0], [1, 0]], "level": l1.0 }),
    )
    .unwrap();
    assert_eq!(first.as_array().unwrap().len(), 2);
    run(
        &r,
        &mut doc,
        "component.place",
        json!({ "library": lib, "item": "Furniture/Seating/stool.obj", "points": [[2, 0]], "level": l1.0, "rotation": "45°" }),
    )
    .unwrap();
    let types = mesh_objects::types(doc.store());
    assert_eq!(types.len(), 1);
    let (tid, t, copies) = &types[0];
    assert_eq!(*copies, 3);
    assert_eq!(t.category, mesh_objects::Category::Furniture);
    assert!((t.size[0] - 0.4).abs() < 1e-9);
    assert_eq!(
        doc.store().get(*tid).unwrap().name.as_deref(),
        Some("Oak Stool")
    );
    assert_eq!(
        t.library.as_ref().unwrap().item,
        "Furniture/Seating/stool.obj"
    );
    // The shrub dropped in by hand places with defaults (no folder settings: metres, generic).
    run(&r, &mut doc, "component.place", json!({ "library": lib, "item": "Planting/box shrub.obj", "points": [[5, 5]], "level": l1.0 })).unwrap();
    assert_eq!(mesh_objects::types(doc.store()).len(), 2);

    // Delete and restore: the file and its MTL move to .deleted, with the entry, and back.
    run(
        &r,
        &mut doc,
        "library.delete_items",
        json!({ "library": lib, "items": ["Furniture/Seating/stool.obj"] }),
    )
    .unwrap();
    assert!(!lib.join("Furniture/Seating/stool.obj").exists());
    assert!(lib.join(".deleted/Furniture/Seating/stool.mtl").is_file());
    let listed = run(
        &r,
        &mut doc,
        "library.list",
        json!({ "library": lib, "folder": "Furniture", "recursive": true }),
    )
    .unwrap();
    assert!(listed["items"].as_array().unwrap().is_empty());
    run(
        &r,
        &mut doc,
        "library.restore_items",
        json!({ "library": lib, "items": ["Furniture/Seating/stool.obj"] }),
    )
    .unwrap();
    let meta = run(
        &r,
        &mut doc,
        "library.get_meta",
        json!({ "library": lib, "item": "Furniture/Seating/stool.obj" }),
    )
    .unwrap();
    assert_eq!(meta["details"]["name"], "Oak Stool");
    assert!(lib.join("Furniture/Seating/stool.mtl").is_file());
}
