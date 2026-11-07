// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The bundled Component Libraries (DM-LIB-006, ARC-FURN-001): `libraries/` holds one library a
//! catalogue library (interiors, gym, retail), each item a model fragment (an IFC file written by
//! Create Component) of one generic furnishing, or a set of them. Every catalogue item is in its
//! library, and every fragment places into a new model as a copy typed for the rule packs.
//!
//! With `RIVIERA_REGEN_LIBRARIES=1` the libraries are rebuilt from the catalogue, keeping each
//! library's id and each item's GUID so placed copies still find their items.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use riviera_app::{App, AppConfig};
use riviera_core::Caller;
use riviera_core::furnishings::{self, Library};
use riviera_core::library::Manifest;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).map(|o| o.result).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries")
}

/// A file name for an item: lower case, letters and digits, words joined by `_`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.replace('×', "x").replace('½', "-half").chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

/// A set: its name, folder, and members (catalogue name, point, rotation in degrees).
struct Set {
    library: Library,
    folder: &'static str,
    name: &'static str,
    description: &'static str,
    members: Vec<(&'static str, [f64; 2], f64)>,
}

fn sets() -> Vec<Set> {
    let inch = 0.0254;
    let td = 30.0 * inch / 2.0;
    let (qw, ql) = (60.0 * inch / 2.0 + 0.05, 80.0 * inch / 2.0 + 0.05);
    let dd = 30.0 * inch / 2.0;
    vec![
        Set {
            library: Library::Interiors,
            folder: "Sets",
            name: "Dining set, 4 seats",
            description: "A 48 × 30 in dining table with two chairs on each long side, pushed in.",
            members: vec![
                ("Dining table, 4 seats 48 × 30 in", [0.0, 0.0], 0.0),
                ("Dining chair", [-0.28, td + 0.1], 0.0),
                ("Dining chair", [0.28, td + 0.1], 0.0),
                ("Dining chair", [-0.28, -td - 0.1], 180.0),
                ("Dining chair", [0.28, -td - 0.1], 180.0),
            ],
        },
        Set {
            library: Library::Interiors,
            folder: "Sets",
            name: "Bedroom set, queen",
            description: "A queen bed with a nightstand each side of its head, 20 mm clear of it.",
            members: vec![
                ("Bed, queen 60 × 80 in", [0.0, 0.0], 0.0),
                ("Nightstand", [-qw - 0.02 - 11.0 * inch, ql - 8.0 * inch], 0.0),
                ("Nightstand", [qw + 0.02 + 11.0 * inch, ql - 8.0 * inch], 0.0),
            ],
        },
        Set {
            library: Library::Interiors,
            folder: "Sets",
            name: "Office workstation",
            description: "A 60 × 30 in desk with its task chair in front of it.",
            members: vec![("Desk 60 × 30 in", [0.0, 0.0], 0.0), ("Task chair", [0.0, -dd - 0.1], 180.0)],
        },
    ]
    .into_iter()
    .inspect(|s| assert!(s.members.iter().all(|(m, _, _)| furnishings::names().contains(m)), "{}", s.name))
    .collect()
}

/// Writes one fragment into a library: the members placed in a new model, then Create Component.
fn write_fragment(dir: &Path, lib: &Path, folder: &str, file: &str, members: &[(&str, [f64; 2], f64)]) -> String {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join(format!("u-{file}")) });
    ok(&mut app, "file.new", json!({}));
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    let names: Vec<&str> = members.iter().map(|m| m.0).collect();
    let types = ok(&mut app, "furnishing.load", json!({ "names": names }));
    let mut ids = Vec::new();
    for (name, at, rot) in members {
        let copy = ok(&mut app, "mesh_object.place", json!({ "type_id": types[*name], "point": at, "level": level, "rotation": format!("{rot}°") }));
        ids.push(copy.as_u64().or_else(|| copy[0].as_u64()).unwrap());
    }
    let made = ok(&mut app, "component.create", json!({ "element_ids": ids, "name": file, "library": lib, "folder": folder, "base_point": [0.0, 0.0] }));
    made["item"].as_str().unwrap().to_owned()
}

fn regenerate() {
    let dir = tempfile::tempdir().unwrap();
    for l in Library::ALL {
        let lib = root().join(l.dir());
        // Keep the library's id and its items' GUIDs.
        let old = Manifest::read(&lib).unwrap();
        let old_guids: BTreeMap<String, String> = old.as_ref().map(|m| m.items.iter().map(|(k, v)| (k.clone(), v.guid.clone())).collect()).unwrap_or_default();
        if lib.exists() {
            for f in walk(&lib) {
                if f.extension().is_some_and(|e| e == "ifc") {
                    std::fs::remove_file(f).unwrap();
                }
            }
            let _ = std::fs::remove_file(lib.join(riviera_core::library::MANIFEST));
        }
        std::fs::create_dir_all(&lib).unwrap();
        let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u-lib") });
        ok(&mut app, "file.new", json!({}));
        ok(&mut app, "library.create", json!({ "path": lib, "name": l.title() }));
        // Each item, then the sets: (path, name, description, tags, sources).
        let mut entries: Vec<(String, String, String, Vec<String>, String)> = Vec::new();
        for i in furnishings::catalogue().into_iter().filter(|i| i.library == l) {
            let item = write_fragment(dir.path(), &lib, i.folder, &slug(i.name), &[(i.name, [0.0, 0.0], 0.0)]);
            let mut tags = vec![i.folder.to_ascii_lowercase()];
            tags.extend(i.kind.map(str::to_owned));
            if i.predefined != "USERDEFINED" {
                tags.push(i.predefined.to_ascii_lowercase());
            }
            tags.extend(i.uniclass.map(|u| format!("Uniclass {u}")));
            entries.push((item, i.name.to_owned(), i.description.to_owned(), tags, i.source.to_owned()));
        }
        for s in sets().into_iter().filter(|s| s.library == l) {
            let item = write_fragment(dir.path(), &lib, s.folder, &slug(s.name), &s.members);
            entries.push((item, s.name.to_owned(), s.description.to_owned(), vec!["set".into()], String::new()));
        }
        let mut m = Manifest::read(&lib).unwrap().expect("made");
        if let Some(o) = &old {
            m.id = o.id.clone();
        }
        m.description = Some(format!(
            "Generic {} that ship with Riviera: placeholders at the sizes research note 15 gives, each a model fragment (IFC) with its IFC class and predefined type or kind, so the rule packs choose it. Generated from riviera_core::furnishings (ARC-FURN-001); regenerate with RIVIERA_REGEN_LIBRARIES=1.",
            match l {
                Library::Interiors => "furniture",
                Library::Gym => "gym equipment",
                Library::Retail => "retail and checkout fixtures",
            }
        ));
        m.author = Some("Riviera contributors".into());
        m.licence = Some("CC0-1.0".into());
        for (path, name, description, tags, sources) in entries {
            let e = m.items.entry(path.clone()).or_default();
            if let Some(g) = old_guids.get(&path) {
                e.guid = g.clone();
            }
            e.name = Some(name);
            e.description = Some(description);
            e.tags = tags;
            let mut prov = BTreeMap::new();
            prov.insert("author".to_owned(), json!("Riviera contributors"));
            prov.insert("licence".to_owned(), json!("CC0-1.0"));
            prov.insert("generated_by".to_owned(), json!("riviera_core::furnishings (ARC-FURN-001)"));
            if !sources.is_empty() {
                prov.insert("sources".to_owned(), json!(sources));
            }
            e.provenance = Some(prov);
        }
        m.write(&lib).unwrap();
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn the_bundled_libraries_hold_every_item_and_each_places_typed() {
    if std::env::var("RIVIERA_REGEN_LIBRARIES").is_ok_and(|v| v == "1") {
        regenerate();
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("u") });
    ok(&mut app, "file.new", json!({}));
    // The three libraries are found and listed.
    let listed = ok(&mut app, "library.bundled", json!({}));
    let names: Vec<&str> = listed.as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    for l in Library::ALL {
        assert!(names.contains(&l.title()), "{} is not bundled: {names:?}", l.title());
    }
    let level = ok(&mut app, "level.list", json!({}))[0]["id"].clone();
    ok(&mut app, "clearance.set_rule_set", json!({ "standard": "ada_2010" }));
    let mut x = 0.0;
    for l in Library::ALL {
        let lib = root().join(l.dir());
        let m = Manifest::read(&lib).unwrap().expect("a manifest");
        assert_eq!(m.licence.as_deref(), Some("CC0-1.0"));
        for i in furnishings::catalogue().into_iter().filter(|i| i.library == l) {
            let path = format!("{}/{}.ifc", i.folder, slug(i.name));
            let e = m.items.get(&path).unwrap_or_else(|| panic!("{} is not in {}: run with RIVIERA_REGEN_LIBRARIES=1", i.name, l.title()));
            assert_eq!(e.name.as_deref(), Some(i.name));
            assert!(lib.join(&path).is_file(), "{path}");
            // Placed far apart, each comes in as one copy of a type with its IFC type or kind.
            x += 6.0;
            let placed = ok(&mut app, "component.place_fragment", json!({ "library": lib, "item": path, "point": [x, 0.0], "level": level }));
            let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
            let copies: Vec<_> = placed.as_array().unwrap().iter().filter_map(|v| store.get(riviera_core::ElementId(v.as_u64()?))).filter(|e| riviera_core::mesh_objects::mesh_object_recipe(e).is_some()).collect();
            assert_eq!(copies.len(), 1, "{}: {placed}", i.name);
            let ty = riviera_core::mesh_objects::type_of(&store, copies[0]).and_then(|(t, _)| store.get(t)).expect("its type");
            assert_eq!(ty.param("@PredefinedType").and_then(Value::as_str), Some(i.predefined), "{}", i.name);
            assert_eq!(ty.param("@ElementType").and_then(Value::as_str), i.kind, "{}", i.name);
            if i.name == "Treadmill, commercial" {
                // Its own 120 V circuit: a power connector for the electrical tools.
                let t = riviera_core::mesh_objects::mesh_object_type(ty).unwrap();
                let volts: Vec<f64> = t.connectors.iter().filter_map(|c| c.electrical.as_ref().map(|e| e.voltage)).collect();
                assert_eq!(volts, [120.0], "{volts:?}");
            }
        }
        for s in sets().into_iter().filter(|s| s.library == l) {
            let path = format!("{}/{}.ifc", s.folder, slug(s.name));
            assert!(m.items.contains_key(&path), "{} is not in {}", s.name, l.title());
            x += 6.0;
            let placed = ok(&mut app, "component.place_fragment", json!({ "library": lib, "item": path, "point": [x, 0.0], "level": level }));
            let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
            let copies = placed.as_array().unwrap().iter().filter_map(|v| store.get(riviera_core::ElementId(v.as_u64()?))).filter(|e| riviera_core::mesh_objects::mesh_object_recipe(e).is_some()).count();
            assert_eq!(copies, s.members.len(), "{}", s.name);
        }
    }
    // Under the ADA rule set the placed beds have their clear spaces, and nothing placed overlaps.
    let zones = ok(&mut app, "clearance.list", json!({}));
    let beds = zones.as_array().unwrap().iter().filter(|z| z["rule"] == "ADA 2010 806.2.3 left").count();
    assert_eq!(beds, 8, "seven beds and the bedroom set's");
    // Nothing in any use zone: the sets keep their own clear spaces free. The only findings are
    // the hanging fixtures placed on the floor, short of ADA's 80 in headroom.
    let found = ok(&mut app, "rules.check", json!({}));
    let hanging = furnishings::catalogue().iter().filter(|i| matches!(i.kind, Some("Chandelier" | "Pendant"))).count();
    assert!(found.as_array().unwrap().iter().all(|f| f["kind"] == "headroom" && f["rule"] == "ADA 2010 307.4"), "{found:#}");
    assert_eq!(found.as_array().unwrap().len(), hanging, "{found:#}");
}
