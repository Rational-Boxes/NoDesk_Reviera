// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! COBie export (DOC-OUT-015): tables from the model, validation, unique names, CSV and XLSX.

use reviera_app::{App, AppConfig};
use reviera_core::{Caller, ElementId};
use serde_json::{Value, json};

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    serde_json::from_value(v).unwrap()
}

/// A CSV file as rows of cells (no quoted commas in these tests' data).
fn csv(path: &std::path::Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| l.split(',').map(str::to_owned).collect())
        .collect()
}

#[test]
fn a_house_exports_as_cobie_with_spaces_types_and_components() {
    let mut app = App::new(AppConfig {
        live: None,
        untitled_dir: std::env::temp_dir(),
    });
    run(&mut app, "file.new", json!({})).unwrap();
    let doc = app.active().unwrap();
    let level = app
        .document(doc)
        .unwrap()
        .doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // Two rooms with a door between them, a door and a window outside.
    let c = [[0.1, 0.1], [9.9, 0.1], [9.9, 5.9], [0.1, 5.9]];
    let mut walls = Vec::new();
    for i in 0..4 {
        walls.push(id(run(
            &mut app,
            "wall.create",
            json!({
                "curve": { "line": { "start": c[(i + 1) % 4], "end": c[i] } },
                "base_level": level.0,
                "top_constraint": { "unconnected": 3 },
            }),
        )
        .unwrap()));
    }
    let partition = id(run(
        &mut app,
        "wall.create",
        json!({
            "curve": { "line": { "start": [5.0, 0.1], "end": [5.0, 5.9] } },
            "base_level": level.0,
            "top_constraint": { "unconnected": 3 },
        }),
    )
    .unwrap());
    run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": walls[0].0, "point_along": 2.0 }),
    )
    .unwrap();
    run(
        &mut app,
        "door.place",
        json!({ "host_wall_id": partition.0, "point_along": 3.0 }),
    )
    .unwrap();
    run(
        &mut app,
        "window.place",
        json!({ "host_wall_id": walls[2].0, "point_along": 3.0 }),
    )
    .unwrap();
    run(
        &mut app,
        "room.place_automatically",
        json!({ "level_id": level.0 }),
    )
    .unwrap();

    // Without a contact, validation says so.
    let issues = run(&mut app, "cobie.validate", json!({})).unwrap();
    assert!(
        issues
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["table"] == "Contact")
    );
    run(
        &mut app,
        "cobie.set_settings",
        json!({ "settings": { "contacts": [{ "email": "bim@example.org", "company": "Studio", "category": "Architect" }] } }),
    )
    .unwrap();
    // Doors and windows number their marks separately: the first door and the window are both
    // "1", which COBie's unique component names do not allow.
    let issues = run(&mut app, "cobie.validate", json!({})).unwrap();
    assert!(
        issues
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["rule"] == "unique_name" && i["record"] == "1")
    );
    let window = app
        .document(doc)
        .unwrap()
        .doc
        .store()
        .iter()
        .find(|e| e.class == "IfcWindow")
        .unwrap()
        .id();
    run(
        &mut app,
        "param.set",
        json!({ "element_ids": [window.0], "param_ref": "elem.mark", "value": "W1" }),
    )
    .unwrap();
    let issues = run(&mut app, "cobie.validate", json!({})).unwrap();
    assert_eq!(issues, json!([]), "a clean model validates");

    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("cobie");
    let out = run(
        &mut app,
        "export.cobie",
        json!({ "path": folder, "format": "csv" }),
    )
    .unwrap();
    let count = |name: &str| {
        out["tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["rows"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(count("Facility"), 1);
    assert_eq!(count("Floor"), 1);
    assert_eq!(count("Space"), 2);
    assert_eq!(count("Component"), 3);
    assert_eq!(count("Type"), 2, "one door type and one window type");
    assert!(count("Attribute") > 0);
    assert_eq!(count("Contact"), 1);
    // Components name their space and type, and carry their GlobalIds.
    let comp = csv(&folder.join("Component.csv"));
    let col = |n: &str| comp[0].iter().position(|c| c == n).unwrap();
    let store = app.document(doc).unwrap().doc.store().clone();
    let spaces: Vec<String> = csv(&folder.join("Space.csv"))[1..]
        .iter()
        .map(|r| r[0].clone())
        .collect();
    for row in &comp[1..] {
        assert!(spaces.contains(&row[col("Space")]), "{row:?}");
        assert_ne!(row[col("TypeName")], "n/a");
        assert_eq!(row[col("CreatedBy")], "bim@example.org");
        let guid = reviera_core::GlobalId::parse(&row[col("ExtIdentifier")]).unwrap();
        let e = store.get(store.find_by_guid(&guid).unwrap()).unwrap();
        assert_eq!(row[col("ExtObject")], e.class);
    }
    // The second door sits between the two rooms; the outer door in the west room.
    let facility = csv(&folder.join("Facility.csv"));
    assert_eq!(
        facility[1][facility[0].iter().position(|c| c == "LinearUnits").unwrap()],
        "meters"
    );
    // Every sheet is written, the empty ones with headers.
    assert!(folder.join("Job.csv").exists() && folder.join("PickLists.csv").exists());
    // The same marks on two doors: reported, or made unique.
    let doors: Vec<ElementId> = store
        .iter()
        .filter(|e| e.class == "IfcDoor")
        .map(|e| e.id())
        .collect();
    run(
        &mut app,
        "param.set",
        json!({ "element_ids": [doors[0].0, doors[1].0], "param_ref": "elem.mark", "value": "=D1" }),
    )
    .unwrap();
    let issues = run(&mut app, "cobie.validate", json!({})).unwrap();
    assert!(
        issues
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["rule"] == "unique_name")
    );
    run(
        &mut app,
        "cobie.set_settings",
        json!({ "settings": { "contacts": [{ "email": "bim@example.org" }], "makeNamesUnique": true } }),
    )
    .unwrap();
    run(
        &mut app,
        "export.cobie",
        json!({ "path": folder, "format": "csv", "overwrite": true }),
    )
    .unwrap();
    let comp = csv(&folder.join("Component.csv"));
    let names: Vec<&str> = comp[1..].iter().map(|r| r[0].as_str()).collect();
    // A name that looks like a formula is kept as text.
    assert!(
        names.contains(&"'=D1") && names.contains(&"'=D1 (2)"),
        "{names:?}"
    );
    // Re-exports differ only in their timestamps.
    let strip = |t: String| {
        t.lines()
            .map(|l| {
                l.split(',')
                    .filter(|c| !c.contains('T') || !c.contains(':'))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let a = strip(std::fs::read_to_string(folder.join("Space.csv")).unwrap());
    run(
        &mut app,
        "export.cobie",
        json!({ "path": folder, "format": "csv", "overwrite": true }),
    )
    .unwrap();
    let b = strip(std::fs::read_to_string(folder.join("Space.csv")).unwrap());
    assert_eq!(a, b);
    // A workbook, which will not overwrite without asking.
    let book = dir.path().join("handover.xlsx");
    run(&mut app, "export.cobie", json!({ "path": book })).unwrap();
    let bytes = std::fs::read(&book).unwrap();
    assert!(
        bytes.starts_with(b"PK") && bytes.len() > 4000,
        "{}",
        bytes.len()
    );
    assert!(run(&mut app, "export.cobie", json!({ "path": book })).is_err());
    // The current layout is refused until it is built.
    assert!(
        run(
            &mut app,
            "export.cobie",
            json!({ "path": dir.path().join("x.xlsx"), "version": "current" })
        )
        .is_err()
    );
}
